//! Translates protocol events into [`ChatChange`]s.

use natizap_protocol::whatsapp_rust::prelude::{Event, EventKind, MessageExt as _, wa};
use natizap_protocol::whatsapp_rust::types::events::{InboundMessage, LazyHistorySync};
use natizap_protocol::{ChatId, Content};

use crate::book::{ChatChange, HistoryChat, Mute, Preview};

/// Protocol events the chat list learns from.
pub(crate) const CHAT_EVENTS: &[EventKind] = &[
    EventKind::HistorySync,
    EventKind::Messages,
    EventKind::PinUpdate,
    EventKind::MuteUpdate,
    EventKind::ArchiveUpdate,
    EventKind::MarkChatAsReadUpdate,
    EventKind::ClearChatUpdate,
    EventKind::DeleteChatUpdate,
    EventKind::ContactUpdate,
    EventKind::PushNameUpdate,
    EventKind::LoggedOut,
];

/// WhatsApp's mute timestamp for "until unmuted".
const MUTED_FOREVER: i64 = -1;

/// Everything `event` says about the chats.
pub(crate) fn changes(event: &Event) -> Vec<ChatChange> {
    let id = |jid: &dyn ToString| ChatId::new(&jid.to_string());
    match event {
        Event::HistorySync(history) => history_changes(history),
        Event::Messages(batch) => batch.messages.iter().flat_map(message_changes).collect(),
        Event::PinUpdate(pin) => {
            let at = pin
                .action
                .pinned
                .unwrap_or(false)
                .then(|| pin.timestamp.timestamp());
            vec![ChatChange::Pinned(id(&pin.jid), at)]
        }
        Event::MuteUpdate(mute) => {
            let state = mute
                .action
                .muted
                .unwrap_or(false)
                .then(|| mute_from(mute.action.mute_end_timestamp.unwrap_or(MUTED_FOREVER)));
            vec![ChatChange::Muted(id(&mute.jid), state)]
        }
        Event::ArchiveUpdate(archive) => {
            vec![ChatChange::Archived(
                id(&archive.jid),
                archive.action.archived.unwrap_or(false),
            )]
        }
        Event::MarkChatAsReadUpdate(read) => {
            vec![ChatChange::Read(
                id(&read.jid),
                read.action.read.unwrap_or(true),
            )]
        }
        Event::ClearChatUpdate(clear) => vec![ChatChange::Cleared(id(&clear.jid))],
        Event::DeleteChatUpdate(delete) => vec![ChatChange::Deleted(id(&delete.jid))],
        Event::ContactUpdate(contact) => {
            let name = contact
                .action
                .full_name
                .clone()
                .or_else(|| contact.action.first_name.clone());
            name.filter(|name| !name.is_empty())
                .map(|name| ChatChange::ContactName(id(&contact.jid), name))
                .into_iter()
                .collect()
        }
        Event::PushNameUpdate(update) if !update.new_push_name.is_empty() => {
            vec![ChatChange::PushName(
                id(&update.jid),
                update.new_push_name.clone(),
            )]
        }
        _ => Vec::new(),
    }
}

fn mute_from(end: i64) -> Mute {
    if end == MUTED_FOREVER {
        Mute::Forever
    } else {
        Mute::Until(end)
    }
}

fn message_changes(inbound: &InboundMessage) -> Vec<ChatChange> {
    let source = &inbound.info.source;
    let chat = ChatId::new(&source.chat.to_string());
    let mut changes = Vec::new();

    let setting = inbound
        .message
        .get_base_message()
        .protocol_message
        .as_option()
        .filter(|protocol| {
            protocol.r#type == Some(wa::message::protocol_message::Type::EphemeralSetting)
        });
    if let Some(setting) = setting {
        let seconds = setting.ephemeral_expiration.filter(|&seconds| seconds > 0);
        changes.push(ChatChange::Disappearing(chat.clone(), seconds));
    }

    if !source.is_group {
        let alternate = if source.is_from_me {
            &source.recipient_alt
        } else {
            &source.sender_alt
        };
        if let Some(alternate) = alternate {
            changes.push(ChatChange::SameChat(
                chat.clone(),
                ChatId::new(&alternate.to_string()),
            ));
        }
        // A business's messages carry the name WhatsApp verified for it, and
        // a "high" level when WhatsApp vouches for it with a seal.
        let business = inbound
            .info
            .verified_name
            .as_ref()
            .and_then(|verified| verified.name.clone())
            .filter(|_| !source.is_from_me && !source.is_group);
        if let Some(name) = business {
            let vouched = inbound.info.verified_level.as_deref() == Some("high");
            changes.push(ChatChange::BusinessName(chat.clone(), name, Some(vouched)));
        }
        if !source.is_from_me && !inbound.info.push_name.is_empty() {
            changes.push(ChatChange::PushName(
                chat.clone(),
                inbound.info.push_name.clone(),
            ));
        }
    }

    if let Some(text) = Content::of(&inbound.message).map(|content| content.summary()) {
        let author = (source.is_group && !source.is_from_me)
            .then(|| inbound.info.push_name.clone())
            .filter(|name| !name.is_empty());
        changes.push(ChatChange::Message(
            chat,
            Preview {
                text,
                sent_at: inbound.info.timestamp.timestamp(),
                from_me: source.is_from_me,
                author,
            },
        ));
    }
    changes
}

fn history_changes(history: &LazyHistorySync) -> Vec<ChatChange> {
    let Some(history) = history.get() else {
        tracing::warn!("could not decode a history sync chunk");
        return Vec::new();
    };
    let mut changes = Vec::new();

    for mapping in &history.phone_number_to_lid_mappings {
        if let (Some(lid), Some(phone)) = (&mapping.lid_jid, &mapping.pn_jid) {
            changes.push(ChatChange::SameChat(ChatId::new(lid), ChatId::new(phone)));
        }
    }
    for conversation in &history.conversations {
        let id = ChatId::new(&conversation.id);
        for other in [
            &conversation.pn_jid,
            &conversation.lid_jid,
            &conversation.new_jid,
        ]
        .into_iter()
        .flatten()
        {
            changes.push(ChatChange::SameChat(id.clone(), ChatId::new(other)));
        }
        // A business's messages carry the name WhatsApp verified for it.
        let business = conversation
            .messages
            .iter()
            .filter_map(|entry| entry.message.as_option())
            .find_map(|info| info.verified_biz_name.clone())
            .filter(|_| !id.is_group());
        if let Some(name) = business {
            changes.push(ChatChange::BusinessName(id.clone(), name, None));
        }
        changes.push(ChatChange::History(
            id.clone(),
            history_chat(conversation, id.is_group()),
        ));
    }
    for pushname in &history.pushnames {
        if let (Some(jid), Some(name)) = (&pushname.id, &pushname.pushname)
            && !name.is_empty()
        {
            changes.push(ChatChange::PushName(ChatId::new(jid), name.clone()));
        }
    }
    changes
}

fn history_chat(conversation: &wa::Conversation, is_group: bool) -> HistoryChat {
    let preview = conversation
        .messages
        .iter()
        .filter_map(|entry| entry.message.as_option())
        .filter_map(|info| {
            let text = Content::of(info.message.as_option()?)?.summary();
            let from_me = info
                .key
                .as_option()
                .and_then(|key| key.from_me)
                .unwrap_or(false);
            let author = (is_group && !from_me)
                .then(|| info.push_name.clone())
                .flatten();
            Some(Preview {
                text,
                sent_at: timestamp(info.message_timestamp),
                from_me,
                author,
            })
        })
        .max_by_key(|preview| preview.sent_at);

    let active_at = timestamp(conversation.conversation_timestamp)
        .max(timestamp(conversation.last_msg_timestamp));
    HistoryChat {
        name: conversation
            .name
            .clone()
            .or_else(|| conversation.display_name.clone()),
        preview,
        active_at,
        unread: conversation.unread_count.unwrap_or(0),
        marked_unread: conversation.marked_as_unread.unwrap_or(false),
        disappearing: conversation
            .ephemeral_expiration
            .filter(|&seconds| seconds > 0),
        pinned_at: conversation.pinned.filter(|&at| at > 0).map(i64::from),
        mute: conversation
            .mute_end_time
            .filter(|&end| end > 0)
            .map(|end| mute_from(i64::try_from(end).unwrap_or(MUTED_FOREVER))),
        archived: conversation.archived.unwrap_or(false),
    }
}

fn timestamp(seconds: Option<u64>) -> i64 {
    seconds.and_then(|s| i64::try_from(s).ok()).unwrap_or(0)
}
