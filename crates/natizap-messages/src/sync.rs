//! Translates protocol events into [`MessageChange`]s.

use natizap_protocol::whatsapp_rust::prelude::{Event, EventKind, MessageExt as _, wa};
use natizap_protocol::whatsapp_rust::types::events::{InboundMessage, LazyHistorySync};
use natizap_protocol::whatsapp_rust::types::presence::ReceiptType;
use natizap_protocol::{ChatId, Content, ContentKind, Extra, MissedCall, Ringing, SealedVote};

use crate::message::{Delivery, Message, MessageChange};

/// Protocol events the message store learns from, besides the calls
/// [`Ringing`] follows.
const MESSAGE_EVENTS: &[EventKind] = &[
    EventKind::HistorySync,
    EventKind::Messages,
    EventKind::Receipt,
    EventKind::LoggedOut,
    EventKind::StarUpdate,
];

/// Translates protocol events, following calls between them to mark the
/// ones nobody answered.
#[derive(Debug, Default)]
pub(crate) struct Sync {
    ringing: Ringing,
}

impl Sync {
    /// Every event kind [`Sync::changes`] reads.
    pub(crate) fn events() -> Vec<EventKind> {
        MESSAGE_EVENTS
            .iter()
            .chain(Ringing::EVENTS)
            .copied()
            .collect()
    }

    /// Everything `event` says about messages.
    pub(crate) fn changes(&mut self, event: &Event) -> Vec<MessageChange> {
        match self.ringing.hear(event) {
            Some(missed) => vec![missed_call(missed)],
            None => changes(event),
        }
    }
}

/// A call nobody answered, as a message in its chat.
fn missed_call(call: MissedCall) -> MessageChange {
    MessageChange::Received(Message {
        chat: call.chat,
        // The call's id: a call reported twice is filed once.
        id: call.id,
        from_me: false,
        sender: call.caller,
        author: None,
        sent_at: call.at,
        kind: if call.video {
            ContentKind::MissedVideoCall
        } else {
            ContentKind::MissedVoiceCall
        },
        text: None,
        quote: None,
        media: None,
        extra: None,
        forwarding: None,
        link: None,
        secret: None,
        media_file: None,
        media_poster: None,
        starred: false,
        sender_phone: None,
        edited: false,
        deleted: false,
        delivery: Delivery::Sent,
    })
}

/// Everything `event` says about messages, calls aside.
fn changes(event: &Event) -> Vec<MessageChange> {
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
            let ids: Vec<String> = receipt
                .message_ids
                .iter()
                .map(ToString::to_string)
                .collect();
            tracing::debug!(?ids, ?delivery, "receipt");
            vec![MessageChange::Progressed { ids, delivery }]
        }
        Event::StarUpdate(update) => vec![MessageChange::Starred {
            message_id: update.message_id.clone(),
            starred: update.action.starred.unwrap_or(false),
        }],
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

    if let Some(vote) = SealedVote::of(inbound) {
        return Some(MessageChange::VoteArrived(vote));
    }

    if let Some(pin) = base.pin_in_chat_message.as_option() {
        use wa::message::pin_in_chat_message::Type;
        let message_id = pin.key.as_option()?.id.clone()?;
        let until = match pin.r#type? {
            Type::PinForAll => {
                // The pin lasts from when it was sent for the length its
                // sender chose; 7 days is WhatsApp's default.
                let since = pin.sender_timestamp_ms.unwrap_or_default() / 1000;
                let length = inbound
                    .message
                    .message_context_info
                    .as_option()
                    .and_then(|context| context.message_add_on_duration_in_secs)
                    .unwrap_or(7 * 86_400);
                Some(since + i64::from(length))
            }
            Type::UnpinForAll => None,
            Type::UnknownType => return None,
        };
        return Some(MessageChange::Pinned { message_id, until });
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
        extra: content.extra,
        forwarding: content.forwarding,
        link: content.link,
        secret: content.secret,
        media_file: None,
        media_poster: None,
        starred: false,
        sender_phone: in_group
            .then_some(source.sender_alt.as_ref())
            .flatten()
            .map(|alt| ChatId::new(&alt.to_string()))
            .filter(|alt| alt.phone_number().is_some()),
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
    let (sync_type, request) = (history.sync_type(), history.peer_data_request_session_id());
    let Some(history) = history.get() else {
        tracing::warn!("could not decode a history sync chunk");
        return Vec::new();
    };
    tracing::info!(
        sync_type,
        request,
        conversations = history.conversations.len(),
        messages = history
            .conversations
            .iter()
            .map(|conversation| conversation.messages.len())
            .sum::<usize>(),
        "history sync arrived"
    );
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
    let content = history_content(chat, info);
    let votes = history_votes(chat, &id, info, content.as_ref());
    // The phone marks the messages it starred in the history too.
    let star = info
        .starred
        .unwrap_or(false)
        .then(|| MessageChange::Starred {
            message_id: id.clone(),
            starred: true,
        });
    content
        .into_iter()
        .chain(reactions)
        .chain(votes)
        .chain(star)
        .collect()
}

/// The votes the phone kept with a poll in the history, already opened.
fn history_votes(
    chat: &ChatId,
    poll_id: &str,
    info: &wa::WebMessageInfo,
    poll: Option<&MessageChange>,
) -> Vec<MessageChange> {
    let Some(MessageChange::Received(Message {
        extra: Some(Extra::Poll(poll)),
        ..
    })) = poll
    else {
        return Vec::new();
    };
    info.poll_updates
        .iter()
        .filter_map(|update| {
            let key = update.poll_update_message_key.as_option()?;
            let vote = update.vote.as_option()?;
            Some(MessageChange::Voted {
                poll_id: poll_id.to_owned(),
                voter: (!key.from_me.unwrap_or(false)).then(|| {
                    key.participant
                        .as_deref()
                        .map_or_else(|| chat.clone(), ChatId::new)
                }),
                options: poll.chosen(&vote.selected_options),
                cast_at: update.sender_timestamp_ms.unwrap_or_default(),
            })
        })
        .collect()
}

/// The content of a missed call the phone kept in the history, which
/// carries no message, only its kind.
fn missed_call_stub(info: &wa::WebMessageInfo) -> Option<Content> {
    use wa::web_message_info::StubType;
    let kind = match info.message_stub_type? {
        StubType::CALL_MISSED_VOICE | StubType::CALL_MISSED_GROUP_VOICE => {
            ContentKind::MissedVoiceCall
        }
        StubType::CALL_MISSED_VIDEO | StubType::CALL_MISSED_GROUP_VIDEO => {
            ContentKind::MissedVideoCall
        }
        _ => return None,
    };
    Some(Content {
        kind,
        text: None,
        quote: None,
        media: None,
        extra: None,
        forwarding: None,
        link: None,
        secret: None,
    })
}

fn history_content(chat: &ChatId, info: &wa::WebMessageInfo) -> Option<MessageChange> {
    let key = info.key.as_option()?;
    let content = match info.message.as_option() {
        Some(message) => Content::of(message)?,
        None => missed_call_stub(info)?,
    };
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
        extra: content.extra,
        forwarding: content.forwarding,
        link: content.link,
        // The history keeps it beside the message too.
        secret: content.secret.or_else(|| info.message_secret.clone()),
        media_file: None,
        media_poster: None,
        starred: false,
        sender_phone: None,
        edited: false,
        deleted: false,
        delivery: if from_me {
            history_delivery(info)
        } else {
            Delivery::Sent
        },
    }))
}

#[cfg(test)]
mod tests {
    use natizap_protocol::whatsapp_rust::chrono::Utc;
    use natizap_protocol::whatsapp_rust::types::events::Receipt;
    use natizap_protocol::whatsapp_rust::types::message::MessageSource;

    use super::*;

    fn receipt(r#type: ReceiptType) -> Event {
        Event::Receipt(
            Receipt::builder()
                .source(MessageSource::default())
                .message_ids(vec!["3EB0A".into(), "3EB0B".into()])
                .timestamp(Utc::now())
                .r#type(r#type)
                .offline(false)
                .build(),
        )
    }

    #[test]
    fn delivery_and_read_receipts_move_sent_messages_forward() {
        let ids = vec!["3EB0A".to_owned(), "3EB0B".to_owned()];
        assert_eq!(
            changes(&receipt(ReceiptType::Delivered)),
            [MessageChange::Progressed {
                ids: ids.clone(),
                delivery: Delivery::Delivered
            }]
        );
        assert_eq!(
            changes(&receipt(ReceiptType::Read)),
            [MessageChange::Progressed {
                ids,
                delivery: Delivery::Read
            }]
        );
        assert_eq!(changes(&receipt(ReceiptType::ReadSelf)), []);
    }

    #[test]
    fn subscribes_to_the_events_it_translates() {
        // Translating an event is useless if the subscription never asks
        // for it.
        assert!(MESSAGE_EVENTS.contains(&receipt(ReceiptType::Read).kind()));
    }
}
