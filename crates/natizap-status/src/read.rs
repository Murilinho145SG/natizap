//! Translates protocol events into status [`Change`]s: posts arriving live
//! and with the history sync, posts deleted, and receipts from people who
//! saw this account's own.

use natizap_protocol::whatsapp_rust::prelude::{Event, EventKind, MessageExt as _, wa};
use natizap_protocol::whatsapp_rust::types::events::InboundMessage;
use natizap_protocol::whatsapp_rust::types::presence::ReceiptType;
use natizap_protocol::whatsapp_rust::wacore_binary::JidExt as _;
use natizap_protocol::{ChatId, Content, ContentKind};

use crate::post::Post;
use crate::store::{Change, Viewer};

/// Protocol events status learns from.
pub(crate) const STATUS_EVENTS: &[EventKind] = &[
    EventKind::Messages,
    EventKind::HistorySync,
    EventKind::Receipt,
];

/// Everything `event` says about status updates.
pub(crate) fn changes(event: &Event) -> Vec<Change> {
    match event {
        Event::Messages(batch) => batch.messages.iter().filter_map(live).collect(),
        Event::HistorySync(history) => history.get().map_or_else(Vec::new, |history| {
            history
                .status_v3_messages
                .iter()
                .filter_map(from_history)
                .collect()
        }),
        Event::Receipt(receipt)
            if receipt.source.chat.is_status_broadcast()
                && matches!(receipt.r#type, ReceiptType::Read | ReceiptType::Played) =>
        {
            let viewer = ChatId::new(&receipt.source.sender.to_non_ad().to_string());
            receipt
                .message_ids
                .iter()
                .map(|id| {
                    Change::Viewed(Viewer {
                        id: id.clone(),
                        viewer: viewer.clone(),
                        seen_at: receipt.timestamp.timestamp(),
                    })
                })
                .collect()
        }
        // This account watched a status on another of its devices.
        Event::Receipt(receipt)
            if receipt.source.chat.is_status_broadcast()
                && matches!(receipt.r#type, ReceiptType::ReadSelf | ReceiptType::PlayedSelf) =>
        {
            receipt
                .message_ids
                .iter()
                .map(|id| Change::SeenElsewhere { id: id.clone() })
                .collect()
        }
        _ => Vec::new(),
    }
}

fn live(inbound: &InboundMessage) -> Option<Change> {
    let source = &inbound.info.source;
    if !source.chat.is_status_broadcast() {
        return None;
    }
    let poster = ChatId::new(&source.sender.to_non_ad().to_string());
    let author = Some(inbound.info.push_name.clone()).filter(|name| !name.is_empty());
    change(
        &inbound.message,
        poster,
        inbound.info.id.clone(),
        inbound.info.timestamp.timestamp(),
        author,
        source.is_from_me,
    )
}

fn from_history(info: &wa::WebMessageInfo) -> Option<Change> {
    let key = info.key.as_option()?;
    let mine = key.from_me.unwrap_or(false);
    let poster = info
        .participant
        .as_deref()
        .or(key.participant.as_deref())
        .map(ChatId::new)?;
    change(
        info.message.as_option()?,
        poster,
        key.id.clone()?,
        info.message_timestamp
            .and_then(|at| i64::try_from(at).ok())
            .unwrap_or(0),
        info.push_name.clone(),
        mine,
    )
}

/// What `message`, posted by `poster`, changes: a new post, or the poster
/// deleting one.
fn change(
    message: &wa::Message,
    poster: ChatId,
    id: String,
    sent_at: i64,
    author: Option<String>,
    mine: bool,
) -> Option<Change> {
    let base = message.get_base_message();
    if let Some(protocol) = base.protocol_message.as_option() {
        use wa::message::protocol_message::Type;
        return (protocol.r#type == Some(Type::REVOKE))
            .then(|| protocol.key.as_option()?.id.clone())
            .flatten()
            .map(|id| Change::Removed { poster, id });
    }
    let content = Content::of(message)?;
    // Status updates are text, photos and videos; the rest (reactions to
    // statuses, for one) has nothing to show here.
    if !matches!(
        content.kind,
        ContentKind::Text | ContentKind::Photo | ContentKind::Video
    ) {
        return None;
    }
    let background = base
        .extended_text_message
        .as_option()
        .and_then(|text| text.background_argb);
    Some(Change::Posted(Box::new(Post {
        poster,
        id,
        sent_at,
        kind: content.kind,
        text: content.text,
        background,
        author,
        media: content.media,
        file: None,
        seen: mine,
        mine,
    })))
}
