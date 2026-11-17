use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Local;
use gpui::{AppContext as _, Context, Entity, EventEmitter, SharedString, Task};
use natizap_protocol::whatsapp_rust::prelude::Event;
use natizap_protocol::{ChatId, ContentKind, Protocol, Quote, Reply, Unread};

use crate::items::{Item, items};
use crate::media::{Fetch, PosterJob, fetch_on_sight, posters_on_sight};
use crate::message::{Delivery, Message, MessageChange};
use crate::store::MessageStore;
use crate::sync::{MESSAGE_EVENTS, changes};

mod downloads;
mod uploads;

/// How many of a chat's newest messages a conversation shows.
const SHOWN_MESSAGES: usize = 400;

/// The chat to show, as the chat list knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenChat {
    /// The address to send to.
    pub id: ChatId,
    /// Every address the chat's messages may be filed under, `id` included.
    pub addresses: Vec<ChatId>,
    /// Name to show in the header.
    pub title: SharedString,
    /// Unread messages to acknowledge when it opens.
    pub unread: u32,
}

/// This computer sent a message, so other views can reflect it right away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageSent {
    /// The chat it went to.
    pub chat: ChatId,
    /// What was sent.
    pub text: String,
    /// When, in Unix seconds.
    pub sent_at: i64,
}

enum Input {
    Event(Arc<Event>),
    Open(OpenChat),
    Apply(Box<MessageChange>),
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
    },
    Unread {
        chat: ChatId,
        messages: Vec<Unread>,
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
    /// Attachments being downloaded, by message id.
    fetching: HashSet<String>,
    /// Videos whose still is being taken, by message id.
    posters: HashSet<String>,
    _tasks: [Task<()>; 3],
}

impl EventEmitter<MessageSent> for MessageLog {}
impl EventEmitter<PlayRequested> for MessageLog {}
impl EventEmitter<SoundReady> for MessageLog {}

/// The user opened a video, now on disk, so the app can play it over the
/// window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayRequested {
    /// The downloaded file.
    pub file: PathBuf,
    /// What to call it while playing.
    pub title: SharedString,
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
        cx: &mut Context<Self>,
    ) -> Self {
        let (inputs_tx, inputs) = async_channel::unbounded();
        let (outputs_tx, outputs) = async_channel::unbounded();

        let events = protocol.read(cx).subscribe(MESSAGE_EVENTS);
        let forward_tx = inputs_tx.clone();
        let forward = cx.background_spawn(async move {
            while let Ok(event) = events.recv().await {
                if forward_tx.send(Input::Event(event)).await.is_err() {
                    break;
                }
            }
        });
        let sync = cx.background_spawn(sync(store_path, inputs, outputs_tx));
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
            fetching: HashSet::new(),
            posters: HashSet::new(),
            _tasks: [forward, sync, apply],
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

    /// Shows `chat` and acknowledges its unread messages.
    pub fn open(&mut self, chat: OpenChat, cx: &mut Context<Self>) {
        if self.open.as_ref().is_some_and(|open| open.id == chat.id) {
            return;
        }
        self.items = Arc::from([]);
        self.open = Some(chat.clone());
        self.send_input(Input::Open(chat));
        cx.notify();
    }

    /// Sends `text` to the open chat, optionally replying to a message. It
    /// shows at once and is marked as failed if WhatsApp does not accept it.
    pub(crate) fn send(&mut self, text: String, reply: Option<Reply>, cx: &mut Context<Self>) {
        let Some((chat, local_id, sent_at)) = self.next_outgoing() else {
            return;
        };
        let mut message = outgoing_message(&chat, &local_id, sent_at, reply.as_ref());
        message.text = Some(text.clone());
        self.send_input(Input::from(MessageChange::Sending(message)));
        cx.emit(MessageSent {
            chat: chat.clone(),
            text: text.clone(),
            sent_at,
        });

        let sending = self.protocol.read(cx).send_text(&chat, text, reply, cx);
        let inputs = self.inputs.clone();
        cx.background_spawn(async move {
            let change = match sending.await {
                Ok(sent) => MessageChange::Accepted {
                    chat,
                    local_id,
                    id: sent.id,
                    media: None,
                },
                Err(error) => {
                    tracing::warn!(%error, "message not sent");
                    MessageChange::Rejected { chat, local_id }
                }
            };
            // Only fails while shutting down, when nothing is left to update.
            let _ = inputs.send(Input::from(change)).await;
        })
        .detach();
    }

    /// The open chat, a fresh local id and the time, for a message about to
    /// be sent; `None` when no chat is open.
    fn next_outgoing(&mut self) -> Option<(ChatId, String, i64)> {
        let chat = self.open.as_ref()?.id.clone();
        self.sent_count += 1;
        let local_id = format!("local-{}-{}", unix_nanos(), self.sent_count);
        Some((chat, local_id, Local::now().timestamp()))
    }

    /// Deletes this account's own message in the open chat for everyone.
    pub(crate) fn delete_for_everyone(&mut self, message_id: String, cx: &mut Context<Self>) {
        let Some(chat) = self.open.as_ref().map(|open| open.id.clone()) else {
            return;
        };
        let deleting = self
            .protocol
            .read(cx)
            .delete_for_everyone(&chat, message_id.clone(), cx);
        self.send_input(Input::from(MessageChange::Deleted {
            chat,
            id: message_id,
        }));
        cx.background_spawn(async move {
            if let Err(error) = deleting.await {
                tracing::warn!(%error, "message not deleted for everyone");
            }
        })
        .detach();
    }

    /// Deletes a message in the open chat from this account's devices.
    /// `sender` wrote it, in groups, unless `from_me`.
    pub(crate) fn delete_for_me(
        &mut self,
        message_id: String,
        from_me: bool,
        sender: Option<&ChatId>,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.open.as_ref().map(|open| open.id.clone()) else {
            return;
        };
        let deleting =
            self.protocol
                .read(cx)
                .delete_for_me(&chat, message_id.clone(), from_me, sender, cx);
        self.send_input(Input::from(MessageChange::Removed {
            chat,
            id: message_id,
        }));
        cx.background_spawn(async move {
            if let Err(error) = deleting.await {
                tracing::warn!(%error, "message not deleted from the other devices");
            }
        })
        .detach();
    }

    /// Reacts to a message in the open chat with `emoji`, or takes this
    /// account's reaction back when `emoji` is `None`.
    pub(crate) fn react(
        &mut self,
        message_id: String,
        sender: Option<ChatId>,
        emoji: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.open.as_ref().map(|open| open.id.clone()) else {
            return;
        };
        self.send_input(Input::from(MessageChange::Reacted {
            chat: chat.clone(),
            message_id: message_id.clone(),
            reactor: None,
            emoji: emoji.clone(),
        }));
        let reacting = self
            .protocol
            .read(cx)
            .react(&chat, message_id, sender, emoji, cx);
        cx.background_spawn(async move {
            if let Err(error) = reacting.await {
                tracing::warn!(%error, "reaction not sent");
            }
        })
        .detach();
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
            } => {
                if self.open.as_ref().is_some_and(|open| open.id == chat) {
                    self.items = items;
                    for fetch in fetch {
                        self.fetch(fetch, false, cx);
                    }
                    for job in posters {
                        self.make_poster(job, cx);
                    }
                    cx.notify();
                }
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

/// Stores incoming changes and rebuilds the open chat's rows when it is
/// affected, off the UI thread.
async fn sync(
    store_path: PathBuf,
    inputs: async_channel::Receiver<Input>,
    outputs: async_channel::Sender<Output>,
) {
    let Some(mut store) = open_store(&store_path) else {
        return;
    };
    let mut open: Option<OpenChat> = None;

    while let Ok(input) = inputs.recv().await {
        let mut refresh = false;
        let mut opened = false;
        let mut arrived: Vec<Unread> = Vec::new();
        let mut next = Some(input);
        while let Some(input) = next.take() {
            match input {
                Input::Open(chat) => {
                    open = Some(chat);
                    opened = true;
                }
                Input::Event(event) if matches!(*event, Event::LoggedOut(_)) => {
                    // Whoever links next may be a different account.
                    log_error(store.clear(), "could not clear the message store");
                    refresh = true;
                }
                Input::Event(event) => {
                    let changes = changes(&event);
                    refresh |= touches(open.as_ref(), &changes);
                    arrived.extend(arrivals(open.as_ref(), &changes));
                    log_error(store.apply(&changes), "could not store messages");
                }
                Input::Apply(change) => {
                    refresh |= touches(open.as_ref(), std::slice::from_ref(&change));
                    log_error(store.apply(&[*change]), "could not store a message");
                }
            }
            next = inputs.try_recv().ok();
        }

        let Some(chat) = open.as_ref() else { continue };
        if opened && chat.unread > 0 {
            match store.latest_incoming(&chat.addresses, chat.unread) {
                Ok(messages) => arrived.extend(messages),
                Err(error) => tracing::error!(%error, "could not read unread messages"),
            }
        }
        if !arrived.is_empty() {
            let output = Output::Unread {
                chat: chat.id.clone(),
                messages: arrived,
            };
            if outputs.send(output).await.is_err() {
                return;
            }
        }
        if opened || refresh {
            let messages = store
                .latest(&chat.addresses, SHOWN_MESSAGES)
                .unwrap_or_else(|error| {
                    tracing::error!(%error, "could not read messages");
                    Vec::new()
                });
            let reactions = store.reactions(&messages).unwrap_or_else(|error| {
                tracing::error!(%error, "could not read reactions");
                HashMap::new()
            });
            let rows = items(&messages, &reactions, &chat.title, Local::now()).into();
            let fetch = fetch_on_sight(&messages);
            let posters = posters_on_sight(&messages);
            if outputs
                .send(Output::Thread {
                    chat: chat.id.clone(),
                    items: rows,
                    fetch,
                    posters,
                })
                .await
                .is_err()
            {
                return;
            }
        }
    }
}

/// Messages others sent to the chat on screen: read as they arrive.
fn arrivals<'a>(
    open: Option<&'a OpenChat>,
    changes: &'a [MessageChange],
) -> impl Iterator<Item = Unread> + 'a {
    changes.iter().filter_map(move |change| match change {
        MessageChange::Received(message)
            if !message.from_me && open?.addresses.contains(&message.chat) =>
        {
            Some(Unread {
                id: message.id.clone(),
                sender: message.sender.clone(),
            })
        }
        _ => None,
    })
}

fn touches(open: Option<&OpenChat>, changes: &[MessageChange]) -> bool {
    open.is_some_and(|open| {
        changes.iter().any(|change| {
            change
                .chat()
                .is_none_or(|chat| open.addresses.contains(chat))
        })
    })
}

fn open_store(path: &Path) -> Option<MessageStore> {
    MessageStore::open(path)
        .or_else(|error| {
            // Conversations still work for this session without the file.
            tracing::error!(%error, path = %path.display(), "could not open the message store");
            MessageStore::in_memory()
        })
        .inspect_err(|error| tracing::error!(%error, "could not create an in-memory store"))
        .ok()
}

fn log_error(result: rusqlite::Result<()>, context: &str) {
    if let Err(error) = result {
        tracing::error!(%error, "{context}");
    }
}

/// A message this computer is sending, as it shows until WhatsApp accepts
/// it; the caller fills in what it carries.
fn outgoing_message(chat: &ChatId, local_id: &str, sent_at: i64, reply: Option<&Reply>) -> Message {
    Message {
        chat: chat.clone(),
        id: local_id.to_owned(),
        from_me: true,
        sender: None,
        author: None,
        sent_at,
        kind: ContentKind::Text,
        text: None,
        edited: false,
        deleted: false,
        delivery: Delivery::Sending,
        media: None,
        media_file: None,
        media_poster: None,
        quote: reply.map(|reply| Quote {
            id: reply.id.clone(),
            sender: reply.sender.clone(),
            summary: Some(reply.text.clone()),
        }),
    }
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos())
}
