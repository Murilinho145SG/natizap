use natizap_protocol::{ChatId, ContentKind, Quote};

/// How far a message this account sent has gone. Received messages are
/// always [`Delivery::Sent`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Delivery {
    /// WhatsApp did not accept it.
    Failed,
    /// Handed to WhatsApp from this computer, not yet accepted.
    Sending,
    /// Accepted by WhatsApp.
    Sent,
    /// Reached the recipient's phone.
    Delivered,
    /// Read (or played) by the recipient.
    Read,
}

impl Delivery {
    /// Every state, in order.
    pub(crate) const ALL: [Self; 5] = [
        Self::Failed,
        Self::Sending,
        Self::Sent,
        Self::Delivered,
        Self::Read,
    ];

    /// Stable name for storage.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Failed => "failed",
            Self::Sending => "sending",
            Self::Sent => "sent",
            Self::Delivered => "delivered",
            Self::Read => "read",
        }
    }

    /// The state stored as `name`.
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|delivery| delivery.name() == name)
    }
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
    /// The message this one replies to.
    pub(crate) quote: Option<Quote>,
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
    /// Messages this account sent got further. Receipts can name the chat by
    /// a different address than the message was filed under, so they match
    /// by id alone; ids are unique.
    Progressed {
        ids: Vec<String>,
        delivery: Delivery,
    },
}

impl MessageChange {
    /// The chat address the change concerns, when it names one.
    pub(crate) fn chat(&self) -> Option<&ChatId> {
        match self {
            Self::Received(message) | Self::Sending(message) => Some(&message.chat),
            Self::Edited { chat, .. }
            | Self::Deleted { chat, .. }
            | Self::Accepted { chat, .. }
            | Self::Rejected { chat, .. } => Some(chat),
            Self::Progressed { .. } => None,
        }
    }
}
