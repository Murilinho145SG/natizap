use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use gpui::{App, AppContext as _, Task};
use gpui_tokio::Tokio;
use whatsapp_rust::prelude::{Client, MessageBuilderExt as _, MessageExt as _, MessageField, wa};
use whatsapp_rust::{Jid, RevokeType};

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

    fn client(&self) -> Result<Arc<Client>, ActionError> {
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
