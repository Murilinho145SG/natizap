use natizap_protocol::{ChatId, ContentKind};

/// Where a message sent from this computer stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Delivery {
    /// Handed to WhatsApp, not yet accepted.
    Sending,
    /// Accepted by WhatsApp. Also every message received or sent elsewhere.
    Sent,
    /// WhatsApp did not accept it.
    Failed,
}

/// One message in a chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Message {
    /// The chat address it was filed under.
    pub(crate) chat: ChatId,
    /// WhatsApp's id, or a local one while sending.
    pub(crate) id: String,
    /// Whether this account sent it.
    pub(crate) from_me: bool,
    /// Who sent it, in groups.
    pub(crate) sender: Option<ChatId>,
    /// The sender's self-chosen name, in groups.
    pub(crate) author: Option<String>,
    /// When it was sent, in Unix seconds.
    pub(crate) sent_at: i64,
    /// What it carries.
    pub(crate) kind: ContentKind,
    /// Text or caption.
    pub(crate) text: Option<String>,
    /// Changed after sending.
    pub(crate) edited: bool,
    /// Deleted for everyone.
    pub(crate) deleted: bool,
    /// Delivery state of messages sent from here.
    pub(crate) delivery: Delivery,
}

/// One fact learned about the messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MessageChange {
    /// A message arrived, was sent elsewhere, or came with a history sync.
    Received(Message),
    /// A message's text was edited.
    Edited {
        chat: ChatId,
        id: String,
        text: Option<String>,
    },
    /// A message was deleted for everyone.
    Deleted { chat: ChatId, id: String },
    /// This computer started sending a message under a local id.
    Sending(Message),
    /// WhatsApp accepted a message and gave it its real id.
    Accepted {
        chat: ChatId,
        local_id: String,
        id: String,
    },
    /// WhatsApp did not accept a message.
    Rejected { chat: ChatId, local_id: String },
}

impl MessageChange {
    /// The chat address the change concerns.
    pub(crate) fn chat(&self) -> &ChatId {
        match self {
            Self::Received(message) | Self::Sending(message) => &message.chat,
            Self::Edited { chat, .. }
            | Self::Deleted { chat, .. }
            | Self::Accepted { chat, .. }
            | Self::Rejected { chat, .. } => chat,
        }
    }
}
