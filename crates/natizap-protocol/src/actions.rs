use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use gpui::{App, AppContext as _, Task};
use gpui_tokio::Tokio;
use whatsapp_rust::prelude::{Client, MessageBuilderExt as _, MessageExt as _, MessageField, wa};
use whatsapp_rust::{Jid, PinDuration, RevokeType};

use crate::ChatId;

/// The client of the current connection, while there is one.
#[derive(Clone, Default)]
pub(crate) struct Connection(Arc<Mutex<Option<Arc<Client>>>>);

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connection").finish_non_exhaustive()
    }
}

impl Connection {
    pub(crate) fn set(&self, client: Option<Arc<Client>>) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = client;
    }

    pub(crate) fn client(&self) -> Result<Arc<Client>, ActionError> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(ActionError::Offline)
    }

    /// Runs `action` with the client on Tokio.
    pub(crate) fn run<R, F>(
        &self,
        cx: &App,
        action: impl FnOnce(Arc<Client>) -> F,
    ) -> Task<Result<R, ActionError>>
    where
        F: Future<Output = Result<R, ActionError>> + Send + 'static,
        R: Send + 'static,
    {
        let client = match self.client() {
            Ok(client) => client,
            Err(error) => return Task::ready(Err(error)),
        };
        let task = Tokio::spawn(cx, action(client));
        cx.background_spawn(async move { task.await.map_err(|_| ActionError::Interrupted)? })
    }
}

/// This account's own addresses: its phone number and its LID.
pub(crate) fn own_addresses(client: &Client) -> Vec<ChatId> {
    [client.pn(), client.lid()]
        .into_iter()
        .flatten()
        .map(|jid| ChatId::new(&jid.to_string()))
        .collect()
}

/// Why an action on WhatsApp did not happen.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ActionError {
    /// There is no connection to WhatsApp right now.
    #[error("not connected to WhatsApp")]
    Offline,
    /// The client stopped before the action finished.
    #[error("the connection closed before WhatsApp answered")]
    Interrupted,
    /// The chat address is not a valid JID.
    #[error("{} is not a valid WhatsApp address", .0.as_str())]
    InvalidAddress(ChatId),
    /// WhatsApp refused the action.
    #[error("WhatsApp refused: {0}")]
    Rejected(String),
}

/// A message this account just sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    /// WhatsApp's id for the message.
    pub id: String,
}

/// An incoming message to acknowledge as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unread {
    /// WhatsApp's id for the message.
    pub id: String,
    /// Who sent it; required in groups, where receipts go to each sender.
    pub sender: Option<ChatId>,
}

pub(crate) fn jid(chat: &ChatId) -> Result<Jid, ActionError> {
    chat.as_str()
        .parse()
        .map_err(|_| ActionError::InvalidAddress(chat.clone()))
}

/// A message history is counted back from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    /// The message's id.
    pub id: String,
    /// This account sent it.
    pub from_me: bool,
    /// When it was sent, in Unix seconds.
    pub sent_at: i64,
}

/// A message to quote when replying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// WhatsApp's id of the quoted message.
    pub id: String,
    /// Who wrote it; `None` for this account's own messages.
    pub sender: Option<ChatId>,
    /// Its text, shown inside the reply on every device.
    pub text: String,
}

pub(crate) async fn send_text(
    client: Arc<Client>,
    to: Jid,
    text: String,
    reply: Option<Reply>,
) -> Result<Sent, ActionError> {
    let mut message = wa::Message::text(text);
    if let Some(reply) = reply {
        message.set_context_info(reply_context(&client, reply)?);
    }
    let sent = client
        .send_message(to, message)
        .await
        .map_err(|error| ActionError::Rejected(error.to_string()))?;
    Ok(Sent {
        id: sent.message_id,
    })
}

/// Deletes this account's own message for everyone in the chat.
pub(crate) async fn delete_for_everyone(
    client: Arc<Client>,
    chat: Jid,
    message_id: String,
) -> Result<(), ActionError> {
    client
        .revoke_message(chat, message_id, RevokeType::Sender)
        .await
        .map_err(|error| ActionError::Rejected(error.to_string()))
}

/// Deletes a message from this account's devices only. `sender` wrote it,
/// in groups; `None` for this account's own and in one-to-one chats.
/// `from_me` says this account wrote it.
pub(crate) async fn delete_for_me(
    client: Arc<Client>,
    chat: Jid,
    message_id: String,
    sender: Option<Jid>,
    from_me: bool,
) -> Result<(), ActionError> {
    client
        .chat_actions()
        .delete_message_for_me(&chat, sender.as_ref(), &message_id, from_me, true, None)
        .await
        .map_err(|error| ActionError::Rejected(error.to_string()))
}

/// Stars or unstars a message on all of this account's devices. `sender`
/// wrote it, in groups; `None` for this account's own and in one-to-one
/// chats.
pub(crate) async fn star(
    client: Arc<Client>,
    chat: Jid,
    message_id: String,
    sender: Option<Jid>,
    from_me: bool,
    starred: bool,
) -> Result<(), ActionError> {
    let actions = client.chat_actions();
    let result = if starred {
        actions
            .star_message(&chat, sender.as_ref(), &message_id, from_me)
            .await
    } else {
        actions
            .unstar_message(&chat, sender.as_ref(), &message_id, from_me)
            .await
    };
    result.map_err(|error| ActionError::Rejected(error.to_string()))
}

/// How long new messages last before they disappear, as WhatsApp offers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disappearing {
    /// 24 hours.
    Day,
    /// 7 days.
    Week,
    /// 90 days.
    Quarter,
}

impl Disappearing {
    /// Every choice, shortest first.
    pub const ALL: [Self; 3] = [Self::Day, Self::Week, Self::Quarter];

    /// The timer in seconds, as WhatsApp stores it.
    #[must_use]
    pub fn seconds(self) -> u32 {
        match self {
            Self::Day => 86_400,
            Self::Week => 7 * 86_400,
            Self::Quarter => 90 * 86_400,
        }
    }

    /// The choice a stored timer stands for, if any.
    #[must_use]
    pub fn from_seconds(seconds: u32) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|choice| choice.seconds() == seconds)
    }

    /// How the choice reads.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Day => "24 hours",
            Self::Week => "7 days",
            Self::Quarter => "90 days",
        }
    }
}

/// Mutes `chat` until unmuted, or unmutes it, on all of this account's
/// devices.
pub(crate) async fn mute(client: Arc<Client>, chat: Jid, muted: bool) -> Result<(), ActionError> {
    let actions = client.chat_actions();
    let result = if muted {
        actions.mute_chat(&chat).await
    } else {
        actions.unmute_chat(&chat).await
    };
    result.map_err(|error| ActionError::Rejected(error.to_string()))
}

/// Makes new messages in `chat` disappear after `timer`, or stops it.
pub(crate) async fn set_disappearing(
    client: Arc<Client>,
    chat: Jid,
    timer: Option<Disappearing>,
) -> Result<(), ActionError> {
    let seconds = timer.map_or(0, Disappearing::seconds);
    let rejected = |error: &dyn std::fmt::Display| ActionError::Rejected(error.to_string());
    // Groups keep their timer on the server; one-to-one chats announce it
    // in a message.
    if chat.to_string().ends_with("@g.us") {
        client
            .groups()
            .set_ephemeral(chat, seconds)
            .await
            .map_err(|error| rejected(&error))
    } else {
        client
            .set_chat_disappearing_timer(chat, seconds)
            .await
            .map(|_| ())
            .map_err(|error| rejected(&error))
    }
}

/// How long a pinned message stays pinned, as WhatsApp offers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinFor {
    /// 24 hours.
    Day,
    /// 7 days.
    Week,
    /// 30 days.
    Month,
}

impl PinFor {
    /// Every choice, shortest first, as the pin question lists them.
    pub const ALL: [Self; 3] = [Self::Day, Self::Week, Self::Month];

    /// How the choice reads.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Day => "24 hours",
            Self::Week => "7 days",
            Self::Month => "30 days",
        }
    }

    /// The length in seconds.
    #[must_use]
    pub fn seconds(self) -> i64 {
        match self {
            Self::Day => 86_400,
            Self::Week => 7 * 86_400,
            Self::Month => 30 * 86_400,
        }
    }
}

/// Pins a message for everyone in the chat for `pin`, or unpins it when
/// `pin` is `None`. `sender` wrote it, in groups; `None` for this account's
/// own and in one-to-one chats.
pub(crate) async fn pin(
    client: Arc<Client>,
    chat: Jid,
    message_id: String,
    sender: Option<Jid>,
    from_me: bool,
    pin: Option<PinFor>,
) -> Result<(), ActionError> {
    let key = wa::MessageKey {
        remote_jid: Some(chat.to_string()),
        from_me: Some(from_me),
        id: Some(message_id),
        participant: sender.map(|sender| sender.to_string()),
    };
    let result = match pin {
        Some(pin) => {
            let duration = match pin {
                PinFor::Day => PinDuration::Hours24,
                PinFor::Week => PinDuration::Days7,
                PinFor::Month => PinDuration::Days30,
            };
            client.pin_message(chat, key, duration).await
        }
        None => client.unpin_message(chat, key).await,
    };
    result.map_err(|error| ActionError::Rejected(error.to_string()))
}

/// What makes a message a reply to `reply`.
pub(crate) fn reply_context(client: &Client, reply: Reply) -> Result<wa::ContextInfo, ActionError> {
    let participant = match &reply.sender {
        Some(sender) => Some(jid(sender)?),
        None => client.pn(),
    };
    Ok(wa::ContextInfo {
        stanza_id: Some(reply.id),
        participant: participant.map(|jid| jid.to_string()),
        quoted_message: MessageField::some(wa::Message::text(reply.text)),
        ..Default::default()
    })
}

pub(crate) async fn react(
    client: Arc<Client>,
    chat: Jid,
    message_id: String,
    sender: Option<ChatId>,
    emoji: Option<String>,
) -> Result<(), ActionError> {
    let from_me = sender.is_none();
    // Only group messages name their author in the key.
    let participant = match &sender {
        Some(sender) if chat.to_string().ends_with("@g.us") => Some(jid(sender)?.to_string()),
        _ => None,
    };
    let reaction = wa::Message {
        reaction_message: MessageField::some(wa::message::ReactionMessage {
            key: MessageField::some(wa::MessageKey {
                remote_jid: Some(chat.to_string()),
                from_me: Some(from_me),
                id: Some(message_id),
                participant,
            }),
            // An empty reaction removes the previous one.
            text: Some(emoji.unwrap_or_default()),
            sender_timestamp_ms: Some(now_ms()),
            ..Default::default()
        }),
        ..Default::default()
    };
    client
        .send_message(chat, reaction)
        .await
        .map_err(|error| ActionError::Rejected(error.to_string()))?;
    Ok(())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
}

pub(crate) async fn mark_read(
    client: Arc<Client>,
    chat: Jid,
    messages: Vec<Unread>,
) -> Result<(), ActionError> {
    // One receipt per sender: WhatsApp attributes each to whoever wrote the
    // messages it covers.
    let mut by_sender: BTreeMap<Option<ChatId>, Vec<String>> = BTreeMap::new();
    for message in messages {
        by_sender
            .entry(message.sender)
            .or_default()
            .push(message.id);
    }
    for (sender, ids) in by_sender {
        let sender = sender.as_ref().map(jid).transpose()?;
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        client
            .mark_as_read(&chat, sender.as_ref(), &ids)
            .await
            .map_err(|error| ActionError::Rejected(error.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_back_every_disappearing_timer() {
        for timer in Disappearing::ALL {
            assert_eq!(Disappearing::from_seconds(timer.seconds()), Some(timer));
        }
        assert_eq!(Disappearing::from_seconds(0), None);
    }
}
