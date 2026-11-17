use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use gpui::{App, AppContext as _, Task};
use gpui_tokio::Tokio;
use whatsapp_rust::Jid;
use whatsapp_rust::prelude::{Client, MessageBuilderExt as _, wa};

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

pub(crate) async fn send_text(
    client: Arc<Client>,
    to: Jid,
    text: String,
) -> Result<Sent, ActionError> {
    let sent = client
        .send_message(to, wa::Message::text(text))
        .await
        .map_err(|error| ActionError::Rejected(error.to_string()))?;
    Ok(Sent {
        id: sent.message_id,
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
