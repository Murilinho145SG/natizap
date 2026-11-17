use std::path::PathBuf;
use std::sync::Arc;

use gpui::{App, Context, EventEmitter, Task};
use gpui_tokio::{JoinError, Tokio};
use whatsapp_rust::prelude::*;
use whatsapp_rust::wacore::store::DevicePropsOverride;

use crate::ChatId;
use crate::actions::{self, ActionError, Connection, Reply, Sent, Unread};
use crate::fanout::Fanout;

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
    _unlinks: Task<()>,
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
        Self {
            store_path,
            fanout,
            connection,
            client,
            unlinked: false,
            _unlinks: unlinks,
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
