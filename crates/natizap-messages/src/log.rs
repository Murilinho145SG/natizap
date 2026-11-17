use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Local;
use gpui::{App, AppContext as _, Context, Entity, EventEmitter, SharedString, Task};
use natizap_protocol::whatsapp_rust::prelude::Event;
use natizap_protocol::{
    Anchor, ChatId, ContentKind, Forward, GroupInfo, Protocol, Quote, Reply, Unread,
};

use crate::items::{Item, Members};
use crate::media::{Fetch, PosterJob};
use crate::message::{Delivery, Message, MessageChange};
use crate::overview::{self, ChatOverview};
use crate::sync::MESSAGE_EVENTS;

mod actions;
mod downloads;
mod uploads;
mod worker;

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
    /// What the open group says about its members.
    Members(ChatId, Members),
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
    /// Attachments being downloaded, by message id.
    fetching: HashSet<String>,
    /// Videos whose still is being taken, by message id.
    posters: HashSet<String>,
    pinned: Option<Pinned>,
    group: Option<GroupInfo>,
    _tasks: [Task<()>; 3],
}

impl EventEmitter<MessageSent> for MessageLog {}
impl EventEmitter<PlayRequested> for MessageLog {}
impl EventEmitter<SoundReady> for MessageLog {}
impl EventEmitter<ForwardRequested> for MessageLog {}

/// The user opened a video, now on disk, so the app can play it over the
/// window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayRequested {
    /// The downloaded file.
    pub file: PathBuf,
    /// What to call it while playing.
    pub title: SharedString,
}

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
        let sync = cx.background_spawn(sync(store_path.clone(), inputs, outputs_tx));
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
            fetching: HashSet::new(),
            posters: HashSet::new(),
            pinned: None,
            group: None,
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
        self.open = Some(chat.clone());
        self.send_input(Input::Open(chat));
        cx.notify();
    }

    /// The media and starred messages of a chat filed under `addresses`,
    /// read off the UI thread.
    pub fn overview(&self, addresses: Vec<ChatId>, cx: &App) -> Task<ChatOverview> {
        let path = self.store_path.clone();
        cx.background_spawn(async move { overview::read(&path, &addresses) })
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
        Some((chat, self.next_local_id(), Local::now().timestamp()))
    }

    /// An id for a message until WhatsApp gives it its own.
    fn next_local_id(&mut self) -> String {
        self.sent_count += 1;
        format!("local-{}-{}", unix_nanos(), self.sent_count)
    }

    /// Asks the app, through `log`, to pick chats to forward `forward` to.
    pub(crate) fn request_forward(
        log: &Entity<Self>,
        forward: Forward,
        file: Option<PathBuf>,
        cx: &mut App,
    ) {
        log.update(cx, |_, cx| cx.emit(ForwardRequested { forward, file }));
    }

    /// Sends `request` to each of `chats`, marked as forwarded. Each copy
    /// shows in its chat at once, before WhatsApp has it.
    pub fn forward(
        &mut self,
        request: &ForwardRequested,
        chats: Vec<ChatId>,
        cx: &mut Context<Self>,
    ) {
        let sent_at = Local::now().timestamp();
        for chat in chats {
            let local_id = self.next_local_id();
            let mut message = outgoing_message(&chat, &local_id, sent_at, None);
            message.kind = request.forward.kind;
            message.text.clone_from(&request.forward.text);
            message.media.clone_from(&request.forward.media);
            message.media_file.clone_from(&request.file);
            self.send_input(Input::from(MessageChange::Sending(message)));
            cx.emit(MessageSent {
                chat: chat.clone(),
                text: request
                    .forward
                    .text
                    .clone()
                    .unwrap_or_else(|| request.forward.kind.label().to_owned()),
                sent_at,
            });
            let sending = self
                .protocol
                .read(cx)
                .forward(&chat, request.forward.clone(), cx);
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
                        tracing::warn!(%error, "message not forwarded");
                        MessageChange::Rejected { chat, local_id }
                    }
                };
                // Only fails while shutting down, when nothing is left to update.
                let _ = inputs.send(Input::from(change)).await;
            })
            .detach();
        }
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
                        self.fetch(fetch, false, cx);
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
                let asking = self
                    .protocol
                    .read(cx)
                    .resend_history(&chat, &before, count, cx);
                cx.background_spawn(async move {
                    if let Err(error) = asking.await {
                        tracing::debug!(%error, "could not ask for the chat's history again");
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
        starred: false,
        sender_phone: None,
        extra: None,
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
