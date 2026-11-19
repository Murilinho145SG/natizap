use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use chrono::Local;
use gpui::{App, AppContext as _, Context, Entity, EventEmitter, SharedString, Task};
use natizap_protocol::whatsapp_rust::prelude::Event;
use natizap_protocol::whatsapp_rust::types::events::EventKind;
use natizap_protocol::{
    Anchor, ChatId, ContentKind, Forward, GroupInfo, PersonName, Protocol, Received, Typing, Unread,
};

use crate::items::{Item, Members};
use crate::library::Library;
use crate::media::{Fetch, PosterJob};
use crate::message::{Message, MessageChange};
use crate::overview::{self, ChatOverview};
use crate::presence::Presence;
use crate::store::MessageStore;
use crate::sync::Sync;
use crate::text_search::Case;

mod actions;
mod backlog;
mod downloads;
mod sending;
mod typing;
mod uploads;
mod worker;

pub use backlog::StoredStatus;
use downloads::{Background, Failed, Then};
pub(crate) use typing::Typer;
pub use typing::TypingLook;

use worker::sync;

/// How many of a chat's newest messages a conversation shows.
const SHOWN_MESSAGES: usize = 400;

/// The chat to show, as the chat list knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenChat {
    /// The address to send to.
    pub id: ChatId,
    /// A business WhatsApp vouches for, shown with a seal.
    pub verified: bool,
    /// Every address the chat's messages may be filed under, `id` included.
    pub addresses: Vec<ChatId>,
    /// Name to show in the header.
    pub title: SharedString,
    /// Unread messages to acknowledge when it opens.
    pub unread: u32,
    /// Notifications silenced.
    pub muted: bool,
}

/// This computer sent a message, so other views can reflect it right away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageSent {
    /// The chat it went to.
    pub chat: ChatId,
    /// What it was: text, a photo, a sticker…
    pub kind: ContentKind,
    /// Its text or caption; empty when it has none.
    pub text: String,
    /// When, in Unix seconds.
    pub sent_at: i64,
}

enum Input {
    Event(Arc<Event>),
    Open(OpenChat),
    Close,
    /// Read the open chat again, to fetch what it shows anew.
    Refresh,
    /// Show the open chat back to messages sent at this Unix second.
    Reach(i64),
    Apply(Box<MessageChange>),
    /// What the open group says about its members.
    Members(ChatId, Members),
    /// What people are called.
    Names(Arc<HashMap<ChatId, PersonName>>),
}

impl From<MessageChange> for Input {
    fn from(change: MessageChange) -> Self {
        Self::Apply(Box::new(change))
    }
}

enum Output {
    Thread {
        chat: ChatId,
        items: Arc<[Item]>,
        fetch: Vec<Fetch>,
        posters: Vec<PosterJob>,
        pinned: Option<Pinned>,
    },
    Unread {
        chat: ChatId,
        messages: Vec<Unread>,
    },
    /// The open chat shows media messages filed without their media: the
    /// phone is asked for them again.
    Resend {
        chat: ChatId,
        before: Anchor,
        count: u8,
    },
}

/// Every message natizap has, and the one conversation on screen.
///
/// Stores history and live messages off the UI thread, keeps the open chat's
/// rows current, and sends messages. Notifies observers when the open chat
/// changes or its rows do.
#[derive(Debug)]
pub struct MessageLog {
    protocol: Entity<Protocol>,
    inputs: async_channel::Sender<Input>,
    open: Option<OpenChat>,
    items: Arc<[Item]>,
    sent_count: u64,
    media_dir: PathBuf,
    store_path: PathBuf,
    /// Attachments being downloaded, by message id, with what to do with
    /// each once it lands.
    fetching: HashMap<String, Vec<Then>>,
    /// Videos whose still is being taken, by message id.
    posters: HashSet<String>,
    /// Downloads and stills that failed, by message id: kept out of the
    /// above until the connection comes back, then tried again.
    failed: Failed,
    /// Attachments downloaded without being asked for, a few at a time.
    background: Background,
    /// How much of each attachment downloading arrived, and its size, by
    /// message id.
    receiving: HashMap<String, (Received, u64)>,
    pinned: Option<Pinned>,
    group: Option<GroupInfo>,
    /// What people are called, by address.
    names: Arc<HashMap<ChatId, PersonName>>,
    /// How the conversation shows who is typing.
    typing_look: TypingLook,
    /// The chat settings file.
    settings_path: PathBuf,
    /// Whether the person in the open one-to-one chat is around.
    seen: Presence,
    /// Who is typing or recording, in every chat.
    typing: Typing,
    _tasks: [Task<()>; 5],
}

impl EventEmitter<MessageSent> for MessageLog {}
impl EventEmitter<SoundReady> for MessageLog {}
impl EventEmitter<ForwardRequested> for MessageLog {}

/// The open chat's pinned message, as the bar over the messages shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pinned {
    pub(crate) id: SharedString,
    pub(crate) text: SharedString,
}

/// The user wants to forward a message: the app asks which chats to send
/// it to, then hands them to [`MessageLog::forward`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardRequested {
    /// What to send again.
    pub forward: Forward,
    /// The file on disk, so forwarded copies show it at once.
    pub file: Option<PathBuf>,
}

/// A voice note or audio file asked to play finished downloading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SoundReady {
    pub(crate) message_id: String,
    pub(crate) file: PathBuf,
}

impl MessageLog {
    /// Follows `protocol`'s message events, storing messages at `store_path`
    /// and downloaded attachments in `media_dir`.
    pub fn new(
        protocol: Entity<Protocol>,
        store_path: PathBuf,
        media_dir: PathBuf,
        settings_path: PathBuf,
        cx: &mut Context<Self>,
    ) -> Self {
        let (inputs_tx, inputs) = async_channel::unbounded();
        let (outputs_tx, outputs) = async_channel::unbounded();

        let events = protocol.read(cx).subscribe(&Sync::events());
        let forward_tx = inputs_tx.clone();
        let forward = cx.background_spawn(async move {
            while let Ok(event) = events.recv().await {
                if forward_tx.send(Input::Event(event)).await.is_err() {
                    break;
                }
            }
        });
        let account = protocol.read(cx).account();
        let sync = cx.background_spawn(sync(store_path.clone(), inputs, outputs_tx, account));
        let presence = protocol.read(cx).subscribe(&[
            EventKind::Presence,
            EventKind::ChatPresence,
            EventKind::Messages,
        ]);
        let seen = cx.spawn(async move |this, cx| {
            while let Ok(event) = presence.recv().await {
                let followed = this.update(cx, |log, cx| {
                    // Every chat's typing is kept, for a chat opened while
                    // someone types in it.
                    let typed = log.typing.hear(&event);
                    let open = log.open.as_ref();
                    let seen = open.is_some_and(|open| log.seen.follow(&event, &open.addresses));
                    if typed || seen {
                        cx.notify();
                    }
                });
                if followed.is_err() {
                    break;
                }
            }
        });
        // Attachments fail while the connection is down, and natizap shows
        // chats before it is up: they are tried again once it is.
        let connects = protocol.read(cx).subscribe(&[EventKind::Connected]);
        let retry = cx.spawn(async move |this, cx| {
            while connects.recv().await.is_ok() {
                if this.update(cx, |log, _| log.retry_failed()).is_err() {
                    break;
                }
            }
        });
        let apply = cx.spawn(async move |this, cx| {
            while let Ok(output) = outputs.recv().await {
                if this.update(cx, |log, cx| log.receive(output, cx)).is_err() {
                    break;
                }
            }
        });

        Self {
            protocol,
            inputs: inputs_tx,
            open: None,
            items: Arc::from([]),
            sent_count: 0,
            media_dir,
            store_path,
            fetching: HashMap::new(),
            posters: HashSet::new(),
            failed: Failed::default(),
            background: Background::default(),
            receiving: HashMap::new(),
            pinned: None,
            group: None,
            names: Arc::default(),
            typing_look: TypingLook::read(&settings_path),
            settings_path,
            seen: Presence::default(),
            typing: Typing::default(),
            _tasks: [forward, sync, apply, seen, retry],
        }
    }

    /// The chat on screen, if any.
    #[must_use]
    pub fn open_chat(&self) -> Option<&OpenChat> {
        self.open.as_ref()
    }

    pub(crate) fn items(&self) -> &Arc<[Item]> {
        &self.items
    }

    /// The open chat's pinned message, if any.
    pub(crate) fn pinned(&self) -> Option<&Pinned> {
        self.pinned.as_ref()
    }

    /// Shows `chat` and acknowledges its unread messages.
    pub fn open(&mut self, chat: OpenChat, cx: &mut Context<Self>) {
        if self.open.as_ref().is_some_and(|open| open.id == chat.id) {
            return;
        }
        self.items = Arc::from([]);
        self.pinned = None;
        self.group = None;
        if chat.id.is_group() {
            self.load_group(chat.id.clone(), cx);
        }
        self.seen = Presence::default();
        if !chat.id.is_group() {
            // The phone-number address: presence is not sent for LIDs.
            let person = chat
                .addresses
                .iter()
                .find(|address| address.phone_number().is_some())
                .unwrap_or(&chat.id);
            let watching = self.protocol.read(cx).watch_presence(person, cx);
            cx.background_spawn(async move {
                if let Err(error) = watching.await {
                    tracing::debug!(%error, "presence not watched");
                }
            })
            .detach();
        }
        self.open = Some(chat.clone());
        self.send_input(Input::Open(chat));
        cx.notify();
    }

    /// Closes the open chat, leaving none.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.open.take().is_none() {
            return;
        }
        self.items = Arc::from([]);
        self.pinned = None;
        self.group = None;
        self.send_input(Input::Close);
        cx.notify();
    }

    /// Records that the open chat's notifications were silenced or not.
    pub fn set_muted(&mut self, muted: bool, cx: &mut Context<Self>) {
        if let Some(open) = &mut self.open {
            open.muted = muted;
            cx.notify();
        }
    }

    /// Whether the person in the open one-to-one chat is online, typing or
    /// recording, or when they were last seen.
    pub(crate) fn presence_line(&self) -> Option<String> {
        let doing = self
            .open
            .as_ref()
            .filter(|open| !open.id.is_group())
            .and_then(|open| {
                self.typing
                    .in_chat(&open.addresses)
                    .first()
                    .map(|(_, doing)| *doing)
            });
        self.seen.label(doing, Local::now())
    }

    /// The media and starred messages of a chat filed under `addresses`,
    /// read off the UI thread.
    pub fn overview(&self, addresses: Vec<ChatId>, cx: &App) -> Task<ChatOverview> {
        let path = self.store_path.clone();
        cx.background_spawn(async move { overview::read(&path, &addresses) })
    }

    /// Shows the open chat back to the messages sent at `sent_at`, so one
    /// sent then can be gone to.
    pub(crate) fn reach(&self, sent_at: i64) {
        self.send_input(Input::Reach(sent_at));
    }

    /// Everything `addresses`' chat shared: media, docs and links. Read off
    /// the UI thread.
    pub(crate) fn library(&self, addresses: Vec<ChatId>, cx: &App) -> Task<Library> {
        let (path, media_dir) = (self.store_path.clone(), self.media_dir.clone());
        cx.background_spawn(async move { Library::read(&path, &media_dir, &addresses) })
    }

    /// The messages holding `query`, newest first, at most `limit`: in
    /// `chats` when given, else in every chat. Read off the UI thread.
    pub(crate) fn search(
        &self,
        chats: Option<Vec<ChatId>>,
        query: String,
        case: Case,
        limit: usize,
        cx: &App,
    ) -> Task<Vec<Message>> {
        let path = self.store_path.clone();
        cx.background_spawn(async move {
            let found = MessageStore::open(&path)
                .and_then(|store| store.search(chats.as_deref(), &query, case, limit));
            found.unwrap_or_else(|error| {
                tracing::warn!(%error, "could not search the messages");
                Vec::new()
            })
        })
    }

    /// Who runs the open group and who may write in it, once known.
    pub(crate) fn group(&self) -> Option<&GroupInfo> {
        self.group.as_ref()
    }

    fn load_group(&mut self, chat: ChatId, cx: &mut Context<Self>) {
        let loading = self.protocol.read(cx).group_info(&chat, cx);
        cx.spawn(async move |this, cx| match loading.await {
            Ok(group) => {
                let _ = this.update(cx, |log, cx| {
                    // Another chat may have opened meanwhile.
                    if log.open.as_ref().is_some_and(|open| open.id == chat) {
                        let members = Members {
                            community_admins: if group.community_announcements {
                                group.admins.clone()
                            } else {
                                Vec::new()
                            },
                            phones: group.phones.iter().cloned().collect(),
                            ..Members::default()
                        };
                        log.send_input(Input::Members(chat, members));
                        log.group = Some(group);
                        cx.notify();
                    }
                });
            }
            Err(error) => tracing::debug!(%error, "group details unavailable"),
        })
        .detach();
    }

    /// Names people by `names`, by address, where their messages do not:
    /// the address book's names, and those people chose that older
    /// messages lack.
    pub fn set_names(&mut self, names: Arc<HashMap<ChatId, PersonName>>, cx: &mut Context<Self>) {
        self.names = Arc::clone(&names);
        self.send_input(Input::Names(names));
        cx.notify();
    }

    /// The open group's members as its header lists them: names saved in
    /// the address book, then names people chose after a `~`, then
    /// numbers, each in order, and this account last.
    pub(crate) fn members_line(&self, cx: &App) -> Option<String> {
        let group = self.group.as_ref()?;
        let protocol = self.protocol.read(cx);
        let mut own = false;
        let mut listed: Vec<(u8, String)> = Vec::new();
        for member in &group.members {
            let addresses = [Some(&member.address), member.phone.as_ref()];
            if addresses
                .iter()
                .flatten()
                .any(|address| protocol.is_own(address))
            {
                own = true;
                continue;
            }
            let known = addresses
                .iter()
                .flatten()
                .find_map(|address| self.names.get(*address));
            listed.push(match known.and_then(|known| known.saved.clone()) {
                Some(saved) => (0, saved),
                None => match known.and_then(|known| known.chosen.clone()) {
                    Some(chosen) => (1, format!("~ {chosen}")),
                    None => (
                        2,
                        addresses
                            .iter()
                            .flatten()
                            .find_map(|address| address.phone_number())
                            .unwrap_or_else(|| member.address.as_str().to_owned()),
                    ),
                },
            });
        }
        listed.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
        });
        let mut line: Vec<String> = listed.into_iter().map(|(_, name)| name).collect();
        if own {
            line.push("You".to_owned());
        }
        Some(line.join(", "))
    }

    fn send_input(&self, input: Input) {
        // The channel is unbounded and the worker lives as long as `self`.
        if self.inputs.try_send(input).is_err() {
            tracing::error!("message worker stopped");
        }
    }

    fn receive(&mut self, output: Output, cx: &mut Context<Self>) {
        match output {
            Output::Thread {
                chat,
                items,
                fetch,
                posters,
                pinned,
            } => {
                if self.open.as_ref().is_some_and(|open| open.id == chat) {
                    self.items = items;
                    self.pinned = pinned;
                    for fetch in fetch {
                        self.fetch(fetch, Then::Keep, cx);
                    }
                    for job in posters {
                        self.make_poster(job, cx);
                    }
                    cx.notify();
                }
            }
            Output::Resend {
                chat,
                before,
                count,
            } => {
                tracing::info!(
                    chat = chat.as_str(),
                    count,
                    "asking the phone for messages filed without media"
                );
                let asking = self
                    .protocol
                    .read(cx)
                    .resend_history(&chat, &before, count, cx);
                cx.background_spawn(async move {
                    if let Err(error) = asking.await {
                        tracing::warn!(%error, "could not ask for the chat's history again");
                    }
                })
                .detach();
            }
            Output::Unread { chat, messages } => {
                let marking = self.protocol.read(cx).mark_read(&chat, messages, cx);
                cx.background_spawn(async move {
                    if let Err(error) = marking.await {
                        tracing::warn!(%error, "could not mark messages as read");
                    }
                })
                .detach();
            }
        }
    }
}
