//! Translates protocol events into [`ChatChange`]s.

use natizap_protocol::whatsapp_rust::prelude::{Event, EventKind, MessageExt as _, wa};
use natizap_protocol::whatsapp_rust::types::events::{InboundMessage, LazyHistorySync};

use crate::book::{ChatChange, ChatId, HistoryChat, Mute, Preview};

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
        if !source.is_from_me && !inbound.info.push_name.is_empty() {
            changes.push(ChatChange::PushName(
                chat.clone(),
                inbound.info.push_name.clone(),
            ));
        }
    }

    if let Some(text) = preview_text(&inbound.message) {
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
            let text = preview_text(info.message.as_option()?)?;
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

/// Text for the chat list, or `None` for messages that are not shown as a
/// message of their own (reactions, edits, protocol housekeeping).
fn preview_text(message: &wa::Message) -> Option<String> {
    let message = message.get_base_message();
    if let Some(text) = message.text_content().or_else(|| message.get_caption()) {
        return Some(text.to_owned());
    }
    let label = if message.image_message.is_set() {
        "📷 Photo"
    } else if message.video_message.is_set() {
        "🎥 Video"
    } else if let Some(audio) = message.audio_message.as_option() {
        if audio.ptt.unwrap_or(false) {
            "🎤 Voice message"
        } else {
            "🎵 Audio"
        }
    } else if message.document_message.is_set() || message.document_with_caption_message.is_set() {
        "📄 Document"
    } else if message.sticker_message.is_set() {
        "Sticker"
    } else if message.location_message.is_set() {
        "📍 Location"
    } else if message.contact_message.is_set() {
        "👤 Contact"
    } else if message.poll_creation_message.is_set()
        || message.poll_creation_message_v2.is_set()
        || message.poll_creation_message_v3.is_set()
    {
        "📊 Poll"
    } else {
        return None;
    };
    Some(label.to_owned())
}
