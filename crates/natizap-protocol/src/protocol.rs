use std::path::PathBuf;
use std::sync::Arc;

use gpui::{App, Context, EventEmitter, Task};
use gpui_tokio::{JoinError, Tokio};
use whatsapp_rust::prelude::*;
use whatsapp_rust::wacore::store::DevicePropsOverride;

use whatsapp_rust::sync_task::MajorSyncTask;
use whatsapp_rust::wacore::appstate::patch_decode::WAPatchName;

use crate::ChatId;
use crate::ContentKind;
use crate::actions::{self, ActionError, Connection, Disappearing, PinFor, Reply, Sent, Unread};
use crate::fanout::Fanout;
use crate::group::{self, GroupInfo};
use crate::media::{self, Media};
use crate::outgoing::{self, Forward, Outgoing, SentMedia};
use crate::profile::{self, Profile};

/// Name shown on the phone under "Linked devices".
const DEVICE_NAME: &str = "natizap";

/// Why the protocol client could not start.
#[derive(Debug, thiserror::Error)]
pub enum StartError {
    /// The local key store could not be opened.
    #[error("could not open the local store at {path}")]
    Store {
        /// Location of the store on disk.
        path: PathBuf,
        /// Underlying storage error.
        #[source]
        source: whatsapp_rust::store::error::StoreError,
    },
    /// The protocol client could not be built.
    #[error("could not start the WhatsApp client")]
    Client(#[source] whatsapp_rust::bot::BotBuilderError),
}

/// The WhatsApp client, running as a linked device.
///
/// Create it with [`Protocol::new`] inside `cx.new`. Features receive protocol
/// events through [`Protocol::subscribe`] and learn about start failures by
/// subscribing to the entity's [`StartError`] events. Dropping it stops the
/// client.
pub struct Protocol {
    store_path: PathBuf,
    fanout: Fanout,
    connection: Connection,
    client: Task<()>,
    /// The phone unlinked this device, so the stored keys are dead.
    unlinked: bool,
    _tasks: [Task<()>; 2],
}

impl EventEmitter<Arc<StartError>> for Protocol {}

impl std::fmt::Debug for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Protocol")
            .field("store_path", &self.store_path)
            .finish_non_exhaustive()
    }
}

impl Protocol {
    /// Starts a client whose keys live in the SQLite database at
    /// `store_path`.
    ///
    /// The client connects once the current app update finishes, so features
    /// set up alongside it receive every event from the first one.
    /// Requires `gpui_tokio::init` to have been called.
    pub fn new(store_path: PathBuf, cx: &mut Context<Self>) -> Self {
        let fanout = Fanout::default();
        let connection = Connection::default();
        let client = start(
            store_path.clone(),
            fanout.clone(),
            connection.clone(),
            Keys::Keep,
            cx,
        );
        let unlinks = fanout.subscribe(EventInterest::of(&[EventKind::LoggedOut]));
        let unlinks = cx.spawn(async move |this, cx| {
            while unlinks.recv().await.is_ok() {
                if this
                    .update(cx, |protocol, _| protocol.unlinked = true)
                    .is_err()
                {
                    break;
                }
            }
        });
        let connects = fanout.subscribe(EventInterest::of(&[EventKind::Connected]));
        let catch_up = cx.spawn(async move |this, cx| {
            // Once per launch: what arrived while natizap could not keep it,
            // e.g. stars from before it read them, is otherwise never sent
            // again.
            if connects.recv().await.is_ok() {
                let refreshing = this.update(cx, |protocol, cx| {
                    protocol.connection.run(cx, |client| async move {
                        refresh_account_state(&client).await;
                        Ok(())
                    })
                });
                if let Ok(refreshing) = refreshing
                    && let Err(error) = refreshing.await
                {
                    tracing::debug!(%error, "account state not refreshed");
                }
            }
        });
        Self {
            store_path,
            fanout,
            connection,
            client,
            unlinked: false,
            _tasks: [unlinks, catch_up],
        }
    }

    /// Delivers every future event of the given kinds, in order, until the
    /// receiver is dropped. Subscriptions survive [`Protocol::restart`].
    #[must_use]
    pub fn subscribe(&self, kinds: &[EventKind]) -> async_channel::Receiver<Arc<Event>> {
        self.fanout.subscribe(EventInterest::of(kinds))
    }

    /// Stops the client and connects again from scratch, e.g. to get fresh
    /// pairing QR codes after the previous ones expired. After the phone
    /// unlinked this device, the old keys are discarded so a new link starts.
    pub fn restart(&mut self, cx: &mut Context<Self>) {
        let keys = if std::mem::take(&mut self.unlinked) {
            Keys::Discard
        } else {
            Keys::Keep
        };
        // Replacing the task drops, and so stops, the running client.
        self.client = start(
            self.store_path.clone(),
            self.fanout.clone(),
            self.connection.clone(),
            keys,
            cx,
        );
    }

    /// Sends a text message to `chat`, optionally replying to another.
    pub fn send_text(
        &self,
        chat: &ChatId,
        text: String,
        reply: Option<Reply>,
        cx: &App,
    ) -> Task<Result<Sent, ActionError>> {
        let to = match actions::jid(chat) {
            Ok(to) => to,
            Err(error) => return Task::ready(Err(error)),
        };
        self.connection.run(cx, move |client| {
            actions::send_text(client, to, text, reply)
        })
    }

    /// Uploads `outgoing` and sends it to `chat`, optionally replying to
    /// another message.
    pub fn send_media(
        &self,
        chat: &ChatId,
        outgoing: Outgoing,
        reply: Option<Reply>,
        cx: &App,
    ) -> Task<Result<SentMedia, ActionError>> {
        let to = match actions::jid(chat) {
            Ok(to) => to,
            Err(error) => return Task::ready(Err(error)),
        };
        self.connection.run(cx, move |client| {
            outgoing::send(client, to, outgoing, reply)
        })
    }

    /// Deletes this account's own message in `chat` for everyone.
    pub fn delete_for_everyone(
        &self,
        chat: &ChatId,
        message_id: String,
        cx: &App,
    ) -> Task<Result<(), ActionError>> {
        let chat = match actions::jid(chat) {
            Ok(chat) => chat,
            Err(error) => return Task::ready(Err(error)),
        };
        self.connection.run(cx, move |client| {
            actions::delete_for_everyone(client, chat, message_id)
        })
    }

    /// Deletes a message in `chat` from this account's devices only, as the
    /// phone's "delete for me". `from_me` says this account wrote it;
    /// `sender` wrote it otherwise, in groups (`None` in one-to-one chats).
    pub fn delete_for_me(
        &self,
        chat: &ChatId,
        message_id: String,
        from_me: bool,
        sender: Option<&ChatId>,
        cx: &App,
    ) -> Task<Result<(), ActionError>> {
        let (chat, sender) = match (actions::jid(chat), sender.map(actions::jid).transpose()) {
            (Ok(chat), Ok(sender)) => (chat, sender),
            (Err(error), _) | (_, Err(error)) => return Task::ready(Err(error)),
        };
        self.connection.run(cx, move |client| {
            actions::delete_for_me(client, chat, message_id, sender, from_me)
        })
    }

    /// Sends `forward` to `chat`, marked as forwarded.
    pub fn forward(
        &self,
        chat: &ChatId,
        forward: Forward,
        cx: &App,
    ) -> Task<Result<Sent, ActionError>> {
        let to = match actions::jid(chat) {
            Ok(to) => to,
            Err(error) => return Task::ready(Err(error)),
        };
        self.connection.run(cx, move |client| async move {
            let id = outgoing::forward(client, to, forward).await?;
            Ok(Sent { id })
        })
    }

    /// Mutes `chat` until unmuted, or unmutes it, on all of this account's
    /// devices.
    pub fn mute(&self, chat: &ChatId, muted: bool, cx: &App) -> Task<Result<(), ActionError>> {
        let jid = match actions::jid(chat) {
            Ok(jid) => jid,
            Err(error) => return Task::ready(Err(error)),
        };
        self.connection
            .run(cx, move |client| actions::mute(client, jid, muted))
    }

    /// Makes new messages in `chat` disappear after `timer`, or stops it.
    pub fn set_disappearing(
        &self,
        chat: &ChatId,
        timer: Option<Disappearing>,
        cx: &App,
    ) -> Task<Result<(), ActionError>> {
        let jid = match actions::jid(chat) {
            Ok(jid) => jid,
            Err(error) => return Task::ready(Err(error)),
        };
        self.connection.run(cx, move |client| {
            actions::set_disappearing(client, jid, timer)
        })
    }

    /// Pins a message in `chat` for everyone for `pin`, or unpins it when
    /// `pin` is `None`. `from_me` says this account wrote it; `sender` wrote
    /// it otherwise, in groups (`None` in one-to-one chats).
    pub fn pin(
        &self,
        chat: &ChatId,
        message_id: String,
        from_me: bool,
        sender: Option<&ChatId>,
        pin: Option<PinFor>,
        cx: &App,
    ) -> Task<Result<(), ActionError>> {
        let (chat, sender) = match (actions::jid(chat), sender.map(actions::jid).transpose()) {
            (Ok(chat), Ok(sender)) => (chat, sender),
            (Err(error), _) | (_, Err(error)) => return Task::ready(Err(error)),
        };
        self.connection.run(cx, move |client| {
            actions::pin(client, chat, message_id, sender, from_me, pin)
        })
    }

    /// Stars or unstars a message in `chat` on all of this account's
    /// devices. `from_me` says this account wrote it; `sender` wrote it
    /// otherwise, in groups (`None` in one-to-one chats).
    pub fn star(
        &self,
        chat: &ChatId,
        message_id: String,
        from_me: bool,
        sender: Option<&ChatId>,
        starred: bool,
        cx: &App,
    ) -> Task<Result<(), ActionError>> {
        let (chat, sender) = match (actions::jid(chat), sender.map(actions::jid).transpose()) {
            (Ok(chat), Ok(sender)) => (chat, sender),
            (Err(error), _) | (_, Err(error)) => return Task::ready(Err(error)),
        };
        self.connection.run(cx, move |client| {
            actions::star(client, chat, message_id, sender, from_me, starred)
        })
    }

    /// Reacts to a message in `chat` with `emoji`, or removes this account's
    /// reaction when `emoji` is `None`. `sender` is who wrote the message;
    /// `None` for this account's own.
    pub fn react(
        &self,
        chat: &ChatId,
        message_id: String,
        sender: Option<ChatId>,
        emoji: Option<String>,
        cx: &App,
    ) -> Task<Result<(), ActionError>> {
        let chat = match actions::jid(chat) {
            Ok(chat) => chat,
            Err(error) => return Task::ready(Err(error)),
        };
        self.connection.run(cx, move |client| {
            actions::react(client, chat, message_id, sender, emoji)
        })
    }

    /// Who runs group `chat` and who may write in it.
    pub fn group_info(&self, chat: &ChatId, cx: &App) -> Task<Result<GroupInfo, ActionError>> {
        let jid = match actions::jid(chat) {
            Ok(jid) => jid,
            Err(error) => return Task::ready(Err(error)),
        };
        self.connection
            .run(cx, move |client| group::info(client, jid))
    }

    /// Downloads the profile picture of `chat`, a person or a group, small
    /// for avatars or `full` size: `None` when it has none or hides it from
    /// this account.
    pub fn profile_picture(
        &self,
        chat: &ChatId,
        full: bool,
        cx: &App,
    ) -> Task<Result<Option<Vec<u8>>, ActionError>> {
        let jid = match actions::jid(chat) {
            Ok(jid) => jid,
            Err(error) => return Task::ready(Err(error)),
        };
        self.connection
            .run(cx, move |client| profile::picture(client, jid, full))
    }

    /// Whether `chat` is this account's own, where people message
    /// themselves. Unknown, so `false`, until the device is linked.
    #[must_use]
    pub fn is_own(&self, chat: &ChatId) -> bool {
        self.connection
            .client()
            .is_ok_and(|client| actions::own_addresses(&client).contains(chat))
    }

    /// What `chat`'s profile shows: their about line, and a business's
    /// public details.
    pub fn profile(&self, chat: &ChatId, cx: &App) -> Task<Result<Profile, ActionError>> {
        let jid = match actions::jid(chat) {
            Ok(jid) => jid,
            Err(error) => return Task::ready(Err(error)),
        };
        self.connection
            .run(cx, move |client| profile::profile(client, jid))
    }

    /// Downloads and decrypts `media`, which a message of `kind` carried.
    pub fn download(
        &self,
        kind: ContentKind,
        media: Media,
        cx: &App,
    ) -> Task<Result<Vec<u8>, ActionError>> {
        self.connection
            .run(cx, move |client| media::download(client, kind, media))
    }

    /// Tells WhatsApp, and so the user's other devices, that `messages` in
    /// `chat` were read.
    pub fn mark_read(
        &self,
        chat: &ChatId,
        messages: Vec<Unread>,
        cx: &App,
    ) -> Task<Result<(), ActionError>> {
        let chat = match actions::jid(chat) {
            Ok(chat) => chat,
            Err(error) => return Task::ready(Err(error)),
        };
        self.connection
            .run(cx, move |client| actions::mark_read(client, chat, messages))
    }
}

/// What to do with the stored keys when starting a client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Keys {
    Keep,
    /// WhatsApp revoked them; reusing them would fail every connection.
    Discard,
}

/// Runs the client on Tokio, starting on the next app tick; start failures
/// are emitted on the entity.
fn start(
    store_path: PathBuf,
    fanout: Fanout,
    connection: Connection,
    keys: Keys,
    cx: &mut Context<Protocol>,
) -> Task<()> {
    cx.spawn(async move |this, cx| {
        let client: Task<Result<Result<(), StartError>, JoinError>> =
            Tokio::spawn(cx, run(store_path, fanout, connection, keys));
        let error = match client.await {
            Ok(Ok(())) => return,
            Ok(Err(error)) => error,
            Err(join_error) => {
                // Only a panic in the client ends up here; cancellation drops
                // this task along with the client.
                tracing::error!(%join_error, "protocol client crashed");
                return;
            }
        };
        tracing::error!(%error, "protocol client failed to start");
        let _ = this.update(cx, |_, cx| cx.emit(Arc::new(error)));
    })
}

async fn run(
    store_path: PathBuf,
    fanout: Fanout,
    connection: Connection,
    keys: Keys,
) -> Result<(), StartError> {
    if keys == Keys::Discard {
        discard_store(&store_path).await;
    }
    let store = SqliteStore::new(&store_path.to_string_lossy())
        .await
        .map_err(|source| StartError::Store {
            path: store_path,
            source,
        })?;

    let bot = Bot::builder()
        .with_backend(store)
        .with_device_props(DevicePropsOverride::new().with_os(DEVICE_NAME))
        .with_event_handler(fanout)
        .build()
        .await
        .map_err(StartError::Client)?;

    connection.set(Some(bot.client()));
    bot.run().await;
    connection.set(None);
    Ok(())
}

/// Asks WhatsApp again for the whole account state the phone keeps in
/// sync, such as stars, mutes, pins and saved names, rather than only what
/// changed. Each arrives as the usual events.
async fn refresh_account_state(client: &Arc<Client>) {
    for name in [
        WAPatchName::CriticalUnblockLow,
        WAPatchName::RegularLow,
        WAPatchName::RegularHigh,
        WAPatchName::Regular,
    ] {
        client
            .process_sync_task(MajorSyncTask::AppStateSync {
                name,
                full_sync: true,
            })
            .await;
    }
}

/// Deletes the SQLite store and its write-ahead log.
async fn discard_store(path: &std::path::Path) {
    for suffix in ["", "-wal", "-shm"] {
        let mut file = path.as_os_str().to_owned();
        file.push(suffix);
        match tokio::fs::remove_file(&file).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::error!(%error, file = %file.display(), "could not delete old keys");
            }
        }
    }
}
