//! Translates protocol events into [`MessageChange`]s.

use natizap_protocol::whatsapp_rust::prelude::{Event, EventKind, MessageExt as _, wa};
use natizap_protocol::whatsapp_rust::types::events::{InboundMessage, LazyHistorySync};
use natizap_protocol::whatsapp_rust::types::presence::ReceiptType;
use natizap_protocol::{ChatId, Content};

use crate::message::{Delivery, Message, MessageChange};

/// Protocol events the message store learns from.
pub(crate) const MESSAGE_EVENTS: &[EventKind] = &[
    EventKind::HistorySync,
    EventKind::Messages,
    EventKind::LoggedOut,
];

/// Everything `event` says about messages.
pub(crate) fn changes(event: &Event) -> Vec<MessageChange> {
    match event {
        Event::HistorySync(history) => history_changes(history),
        Event::Messages(batch) => batch.messages.iter().filter_map(live_change).collect(),
        Event::Receipt(receipt) => {
            let delivery = match receipt.r#type {
                ReceiptType::Delivered => Delivery::Delivered,
                ReceiptType::Read | ReceiptType::Played => Delivery::Read,
                // Receipts about this account's own reading, retries and
                // errors say nothing about where a sent message is.
                _ => return Vec::new(),
            };
            let ids = receipt
                .message_ids
                .iter()
                .map(ToString::to_string)
                .collect();
            vec![MessageChange::Progressed { ids, delivery }]
        }
        _ => Vec::new(),
    }
}

fn live_change(inbound: &InboundMessage) -> Option<MessageChange> {
    let source = &inbound.info.source;
    let chat = ChatId::new(&source.chat.to_string());
    let base = inbound.message.get_base_message();

    if let Some(reaction) = base.reaction_message.as_option() {
        return Some(MessageChange::Reacted {
            chat,
            message_id: reaction.key.as_option()?.id.clone()?,
            reactor: (!source.is_from_me).then(|| ChatId::new(&source.sender.to_string())),
            emoji: reaction.text.clone(),
        });
    }

    if let Some(protocol) = base.protocol_message.as_option() {
        use wa::message::protocol_message::Type;
        let id = protocol.key.as_option()?.id.clone()?;
        return match protocol.r#type? {
            Type::REVOKE => Some(MessageChange::Deleted { chat, id }),
            Type::MESSAGE_EDIT => {
                let text = protocol
                    .edited_message
                    .as_option()
                    .and_then(Content::of)?
                    .text;
                Some(MessageChange::Edited { chat, id, text })
            }
            _ => None,
        };
    }

    let content = Content::of(&inbound.message)?;
    let in_group = source.is_group && !source.is_from_me;
    Some(MessageChange::Received(Message {
        chat,
        id: inbound.info.id.clone(),
        from_me: source.is_from_me,
        sender: in_group.then(|| ChatId::new(&source.sender.to_string())),
        author: in_group
            .then(|| inbound.info.push_name.clone())
            .filter(|name| !name.is_empty()),
        sent_at: inbound.info.timestamp.timestamp(),
        kind: content.kind,
        text: content.text,
        quote: content.quote,
        media: content.media,
        media_file: None,
        edited: false,
        deleted: false,
        delivery: Delivery::Sent,
    }))
}

fn history_delivery(info: &wa::WebMessageInfo) -> Delivery {
    use wa::web_message_info::Status;
    match info.status {
        Some(Status::DELIVERY_ACK) => Delivery::Delivered,
        Some(Status::READ | Status::PLAYED) => Delivery::Read,
        _ => Delivery::Sent,
    }
}

fn history_changes(history: &LazyHistorySync) -> Vec<MessageChange> {
    let Some(history) = history.get() else {
        tracing::warn!("could not decode a history sync chunk");
        return Vec::new();
    };
    history
        .conversations
        .iter()
        .flat_map(|conversation| {
            let chat = ChatId::new(&conversation.id);
            conversation
                .messages
                .iter()
                .filter_map(|entry| entry.message.as_option())
                .flat_map(move |info| history_message(&chat, info))
        })
        .collect()
}

fn history_message(chat: &ChatId, info: &wa::WebMessageInfo) -> Vec<MessageChange> {
    let Some(id) = info.key.as_option().and_then(|key| key.id.clone()) else {
        return Vec::new();
    };
    let reactions = info.reactions.iter().filter_map(|reaction| {
        let key = reaction.key.as_option()?;
        let reactor = if key.from_me.unwrap_or(false) {
            None
        } else {
            Some(
                key.participant
                    .as_deref()
                    .map_or_else(|| chat.clone(), ChatId::new),
            )
        };
        Some(MessageChange::Reacted {
            chat: chat.clone(),
            message_id: id.clone(),
            reactor,
            emoji: reaction.text.clone(),
        })
    });
    history_content(chat, info)
        .into_iter()
        .chain(reactions)
        .collect()
}

fn history_content(chat: &ChatId, info: &wa::WebMessageInfo) -> Option<MessageChange> {
    let key = info.key.as_option()?;
    let content = Content::of(info.message.as_option()?)?;
    let from_me = key.from_me.unwrap_or(false);
    let in_group = chat.is_group() && !from_me;
    let sender = info.participant.as_ref().or(key.participant.as_ref());
    Some(MessageChange::Received(Message {
        chat: chat.clone(),
        id: key.id.clone()?,
        from_me,
        sender: sender.filter(|_| in_group).map(|jid| ChatId::new(jid)),
        author: info.push_name.clone().filter(|_| in_group),
        sent_at: info
            .message_timestamp
            .and_then(|s| i64::try_from(s).ok())
            .unwrap_or(0),
        kind: content.kind,
        text: content.text,
        quote: content.quote,
        media: content.media,
        media_file: None,
        edited: false,
        deleted: false,
        delivery: if from_me {
            history_delivery(info)
        } else {
            Delivery::Sent
        },
    }))
}
