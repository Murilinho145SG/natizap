use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use gpui::{App, AppContext as _, Task};
use gpui_tokio::Tokio;
use whatsapp_rust::prelude::{Client, MessageBuilderExt as _, MessageExt as _, MessageField, wa};
use whatsapp_rust::{Jid, MessageId, MessageRef, PinDuration, RevokeType};

use crate::ChatId;
use crate::content::{Card, Extra};

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
        match self.client() {
            Ok(client) => on_tokio(cx, action(client)),
            Err(error) => Task::ready(Err(error)),
        }
    }
}

/// Runs `work` on Tokio, where the client's own work runs.
pub(crate) fn on_tokio<R, F>(cx: &App, work: F) -> Task<Result<R, ActionError>>
where
    F: Future<Output = Result<R, ActionError>> + Send + 'static,
    R: Send + 'static,
{
    let task = Tokio::spawn(cx, work);
    cx.background_spawn(async move { task.await.map_err(|_| ActionError::Interrupted)? })
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
    /// WhatsApp refused for being asked too often; asking again a little
    /// later works.
    #[error("WhatsApp is busy: asked too often")]
    Busy,
}

impl ActionError {
    /// What a refusal from the client means: [`ActionError::Busy`] for
    /// WhatsApp's rate limit, else [`ActionError::Rejected`].
    pub(crate) fn refused(error: &dyn std::fmt::Display) -> Self {
        let said = error.to_string();
        // The client reports the server's error code only in its message:
        // "received a server error response: code=429, text='rate-overlimit'".
        if said.contains("code=429") || said.contains("rate-overlimit") {
            Self::Busy
        } else {
            Self::Rejected(said)
        }
    }
}

/// A message this account just sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    /// WhatsApp's id for the message.
    pub id: String,
    /// The secret votes on a poll are sealed with; only polls have one.
    pub secret: Option<Vec<u8>>,
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
    /// It is a status update: the reply goes to its poster's chat, quoting
    /// it from their status.
    pub from_status: bool,
}

pub(crate) async fn send_text(
    client: Arc<Client>,
    to: Jid,
    text: String,
    reply: Option<Reply>,
    mentions: Vec<String>,
) -> Result<Sent, ActionError> {
    let mut message = wa::Message::text(text);
    let context = match reply {
        Some(reply) => Some(reply_context(&client, reply)?),
        None => (!mentions.is_empty()).then(wa::ContextInfo::default),
    };
    if let Some(mut context) = context {
        context.mentioned_jid = mentions;
        message.set_context_info(context);
    }
    let sent = client
        .send_message(to, message)
        .await
        .map_err(|error| ActionError::Rejected(error.to_string()))?;
    Ok(Sent {
        id: sent.message_id.into_string(),
        secret: None,
    })
}

/// Sends shared contacts or a poll to `to`; buttons cannot be sent.
pub(crate) async fn send_extra(
    client: Arc<Client>,
    to: Jid,
    extra: Extra,
) -> Result<Sent, ActionError> {
    let rejected = |error: &dyn std::fmt::Display| ActionError::Rejected(error.to_string());
    let message = match extra {
        // Only WhatsApp's business platform may send these; people's
        // clients, this one included, only receive them.
        Extra::Buttons(_) => {
            return Err(ActionError::Rejected(
                "messages with buttons come only from businesses".into(),
            ));
        }
        Extra::Poll(poll) => {
            // How many answers one person may choose: all of them, or one.
            let selectable = if poll.multiple {
                u32::try_from(poll.options.len()).unwrap_or(u32::MAX)
            } else {
                1
            };
            let (sent, _, secret) = client
                .polls()
                .create(to, &poll.question, &poll.options, selectable)
                .await
                .map_err(|error| rejected(&error))?
                .into_parts();
            return Ok(Sent {
                id: sent.message_id.into_string(),
                secret: Some(secret.into_bytes().to_vec()),
            });
        }
        Extra::Cards(cards) => {
            let contact = |card: &Card| {
                let mut contact = wa::message::ContactMessage::default();
                contact.display_name = Some(card.name.clone());
                contact.vcard = Some(vcard(card));
                contact
            };
            let mut message = wa::Message::default();
            match cards.as_slice() {
                [card] => message.contact_message = MessageField::some(contact(card)),
                cards => {
                    let mut array = wa::message::ContactsArrayMessage::default();
                    array.display_name = Some(format!("{} contacts", cards.len()));
                    array.contacts = cards.iter().map(contact).collect();
                    message.contacts_array_message = MessageField::some(array);
                }
            }
            message
        }
    };
    let sent = client
        .send_message(to, message)
        .await
        .map_err(|error| rejected(&error))?;
    Ok(Sent {
        id: sent.message_id.into_string(),
        secret: None,
    })
}

/// The vCard WhatsApp sends for a contact: its name and number, with the
/// `waid` that makes the number open a chat.
fn vcard(card: &Card) -> String {
    let name = card.name.replace(['\n', '\r', ';'], " ");
    let phone = card.phone.as_deref().map_or_else(String::new, |digits| {
        format!("TEL;type=CELL;waid={digits}:+{digits}\n")
    });
    format!("BEGIN:VCARD\nVERSION:3.0\nN:;{name};;;\nFN:{name}\n{phone}END:VCARD")
}

/// Deletes this account's own message for everyone in the chat.
pub(crate) async fn delete_for_everyone(
    client: Arc<Client>,
    chat: Jid,
    message_id: String,
) -> Result<(), ActionError> {
    client
        .revoke_message_raw(chat, message_id, RevokeType::Sender)
        .await
        .map(drop)
        .map_err(|error| ActionError::Rejected(error.to_string()))
}

/// The client's handle on a message: its chat, id and author.
pub(crate) fn message_ref<'a>(
    chat: &'a Jid,
    message_id: &str,
    sender: Option<&'a Jid>,
    from_me: bool,
) -> Result<MessageRef<'a>, ActionError> {
    MessageId::new(message_id)
        .and_then(|id| MessageRef::new(chat, id, sender, from_me))
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
    let target = message_ref(&chat, &message_id, sender.as_ref(), from_me)?;
    client
        .chat_actions()
        .delete_message_for_me(&target, true, None)
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
    let target = message_ref(&chat, &message_id, sender.as_ref(), from_me)?;
    let actions = client.chat_actions();
    let result = if starred {
        actions.star_message(&target).await
    } else {
        actions.unstar_message(&target).await
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
    let mut key = wa::MessageKey::default();
    key.remote_jid = Some(chat.to_string());
    key.from_me = Some(from_me);
    key.id = Some(message_id);
    key.participant = sender.map(|sender| sender.to_string());
    let result = match pin {
        Some(pin) => {
            let duration = match pin {
                PinFor::Day => PinDuration::Hours24,
                PinFor::Week => PinDuration::Days7,
                PinFor::Month => PinDuration::Days30,
            };
            client.pin_message_raw(chat, key, duration).await
        }
        None => client.unpin_message_raw(chat, key).await,
    };
    result
        .map(drop)
        .map_err(|error| ActionError::Rejected(error.to_string()))
}

/// What makes a message a reply to `reply`.
pub(crate) fn reply_context(client: &Client, reply: Reply) -> Result<wa::ContextInfo, ActionError> {
    let participant = match &reply.sender {
        Some(sender) => Some(jid(sender)?),
        None => client.pn(),
    };
    let mut context = wa::ContextInfo::default();
    context.stanza_id = Some(reply.id);
    context.participant = participant.map(|jid| jid.to_string());
    context.quoted_message = MessageField::some(wa::Message::text(reply.text));
    // A status is quoted from the status chat, not the reply's own.
    context.remote_jid = reply
        .from_status
        .then(|| Jid::status_broadcast().to_string());
    Ok(context)
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
    let mut key = wa::MessageKey::default();
    key.remote_jid = Some(chat.to_string());
    key.from_me = Some(from_me);
    key.id = Some(message_id);
    key.participant = participant;
    let mut reaction_message = wa::message::ReactionMessage::default();
    reaction_message.key = MessageField::some(key);
    // An empty reaction removes the previous one.
    reaction_message.text = Some(emoji.unwrap_or_default());
    reaction_message.sender_timestamp_ms = Some(now_ms());
    let mut reaction = wa::Message::default();
    reaction.reaction_message = MessageField::some(reaction_message);
    client
        .send_message(chat, reaction)
        .await
        .map_err(|error| ActionError::Rejected(error.to_string()))?;
    Ok(())
}

/// Now, in Unix milliseconds, as WhatsApp stamps actions.
pub(crate) fn now_ms() -> i64 {
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
