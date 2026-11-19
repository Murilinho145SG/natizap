//! Sending text, contacts, polls and forwards, and settling what WhatsApp
//! answers.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Local;
use gpui::{App, AppContext as _, Context, Entity, Task};
use natizap_protocol::{
    ActionError, Answer, ChatId, ContentKind, Extra, Forward, Quote, Reply, Sent, Sticker,
};

use super::{ForwardRequested, Input, MessageLog, MessageSent};
use crate::message::{Delivery, Message, MessageChange};

impl MessageLog {
    /// Sends `text` to the open chat, optionally replying to a message. It
    /// shows at once and is marked as failed if WhatsApp does not accept it.
    pub fn send(&mut self, text: String, reply: Option<Reply>, cx: &mut Context<Self>) {
        let Some((chat, local_id, sent_at)) = self.next_outgoing() else {
            return;
        };
        let mut message = outgoing_message(&chat, &local_id, sent_at, reply.as_ref());
        message.text = Some(text.clone());
        self.send_input(Input::from(MessageChange::Sending(message)));
        cx.emit(MessageSent {
            chat: chat.clone(),
            kind: ContentKind::Text,
            text: text.clone(),
            sent_at,
        });

        let sending = self.protocol.read(cx).send_text(&chat, text, reply, cx);
        self.settle(chat, local_id, sending, cx);
    }

    /// Answers `offered`, a business's message in the open chat, by tapping
    /// its quick-reply button `answer`; the label shows as a reply to it.
    pub fn send_answer(
        &mut self,
        label: String,
        answer: Answer,
        offered: Reply,
        cx: &mut Context<Self>,
    ) {
        let Some((chat, local_id, sent_at)) = self.next_outgoing() else {
            return;
        };
        let mut message = outgoing_message(&chat, &local_id, sent_at, Some(&offered));
        message.text = Some(label.clone());
        self.send_input(Input::from(MessageChange::Sending(message)));
        cx.emit(MessageSent {
            chat: chat.clone(),
            kind: ContentKind::Text,
            text: label.clone(),
            sent_at,
        });
        let sending = self
            .protocol
            .read(cx)
            .send_answer(&chat, label, answer, offered, cx);
        self.settle(chat, local_id, sending, cx);
    }

    /// Sends shared contacts or a poll to the open chat.
    pub fn send_extra(&mut self, extra: Extra, cx: &mut Context<Self>) {
        let Some((chat, local_id, sent_at)) = self.next_outgoing() else {
            return;
        };
        let mut message = outgoing_message(&chat, &local_id, sent_at, None);
        message.kind = match extra {
            Extra::Cards(_) => ContentKind::Contact,
            Extra::Poll(_) => ContentKind::Poll,
            Extra::Buttons(_) => ContentKind::Text,
        };
        message.extra = Some(extra.clone());
        cx.emit(MessageSent {
            chat: chat.clone(),
            kind: message.kind,
            text: message.text.clone().unwrap_or_default(),
            sent_at,
        });
        self.send_input(Input::from(MessageChange::Sending(message)));
        let sending = self.protocol.read(cx).send_extra(&chat, extra, cx);
        self.settle(chat, local_id, sending, cx);
    }

    /// Files what becomes of the message sent as `local_id` once WhatsApp
    /// answers `sending`.
    pub(super) fn settle(
        &self,
        chat: ChatId,
        local_id: String,
        sending: Task<Result<Sent, ActionError>>,
        cx: &mut Context<Self>,
    ) {
        let inputs = self.inputs.clone();
        cx.background_spawn(async move {
            let change = match sending.await {
                Ok(sent) => MessageChange::Accepted {
                    chat,
                    local_id,
                    id: sent.id,
                    media: None,
                    secret: sent.secret,
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
    pub(super) fn next_outgoing(&mut self) -> Option<(ChatId, String, i64)> {
        let chat = self.open.as_ref()?.id.clone();
        Some((chat, self.next_local_id(), Local::now().timestamp()))
    }

    /// An id for a message until WhatsApp gives it its own.
    pub(super) fn next_local_id(&mut self) -> String {
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
                kind: request.forward.kind,
                text: request.forward.text.clone().unwrap_or_default(),
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
                        secret: sent.secret,
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

    /// Sends `sticker` to the open chat, showing it at once from `file`,
    /// where it already is on disk.
    pub fn send_sticker(
        &mut self,
        sticker: Sticker,
        file: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let Some((chat, local_id, sent_at)) = self.next_outgoing() else {
            return;
        };
        let mut message = outgoing_message(&chat, &local_id, sent_at, None);
        message.kind = ContentKind::Sticker;
        message.media = Some(sticker.media.clone());
        message.media_file.clone_from(&file);
        self.send_input(Input::from(MessageChange::Sending(message)));
        cx.emit(MessageSent {
            chat: chat.clone(),
            kind: ContentKind::Sticker,
            text: String::new(),
            sent_at,
        });
        let sending = self
            .protocol
            .read(cx)
            .send_sticker(&chat, sticker, file, cx);
        let inputs = self.inputs.clone();
        cx.background_spawn(async move {
            let change = match sending.await {
                // Gone up again when its link had expired: it now lives
                // somewhere else.
                Ok(sent) => MessageChange::Accepted {
                    chat,
                    local_id,
                    id: sent.id,
                    media: Some(sent.media),
                    secret: None,
                },
                Err(error) => {
                    tracing::warn!(%error, "sticker not sent");
                    MessageChange::Rejected { chat, local_id }
                }
            };
            // Only fails while shutting down, when nothing is left to update.
            let _ = inputs.send(Input::from(change)).await;
        })
        .detach();
    }
}

/// A message this computer is sending, as it shows until WhatsApp accepts
/// it; the caller fills in what it carries.
pub(super) fn outgoing_message(
    chat: &ChatId,
    local_id: &str,
    sent_at: i64,
    reply: Option<&Reply>,
) -> Message {
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
        forwarding: None,
        link: None,
        secret: None,
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
