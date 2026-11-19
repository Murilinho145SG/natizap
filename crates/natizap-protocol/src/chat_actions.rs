//! What the chat list does to a whole chat, on all of this account's
//! devices: pinning, archiving, marking read, clearing, deleting, and
//! blocking the person behind it.

use gpui::{App, Task};
use whatsapp_rust::{Jid, message_range};

use crate::actions::jid;
use crate::{ActionError, ChatId, Protocol};

/// A change to a whole chat, as the chat list offers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatAction {
    /// Pins it to the top (`true`) or unpins it.
    Pin(bool),
    /// Archives it (`true`) or brings it back.
    Archive(bool),
    /// Marks it read (`true`) or unread.
    MarkRead(bool),
    /// Removes its messages, starred ones aside; the chat stays.
    Clear,
    /// Removes the chat and its messages.
    Delete,
}

impl Protocol {
    /// Applies `action` to `chat` on all of this account's devices.
    /// `last_message_at` (Unix seconds) is when its latest message was
    /// sent: it tells the other devices which messages the action covers.
    pub fn chat_action(
        &self,
        chat: &ChatId,
        action: ChatAction,
        last_message_at: Option<i64>,
        cx: &App,
    ) -> Task<Result<(), ActionError>> {
        let jid = match jid(chat) {
            Ok(jid) => jid,
            Err(error) => return Task::ready(Err(error)),
        };
        let range = last_message_at.map(|at| message_range(at, None, Vec::new()));
        self.connection.run(cx, move |client| async move {
            let actions = client.chat_actions();
            let result = match action {
                ChatAction::Pin(true) => actions.pin_chat(&jid).await,
                ChatAction::Pin(false) => actions.unpin_chat(&jid).await,
                ChatAction::Archive(true) => actions.archive_chat(&jid, range).await,
                ChatAction::Archive(false) => actions.unarchive_chat(&jid, range).await,
                ChatAction::MarkRead(read) => actions.mark_chat_as_read(&jid, read, range).await,
                ChatAction::Clear => actions.clear_chat(&jid, false, false, range).await,
                ChatAction::Delete => actions.delete_chat(&jid, false, range).await,
            };
            result.map_err(|error| ActionError::Rejected(error.to_string()))
        })
    }

    /// Blocks the person at `chat` (`true`) or unblocks them.
    pub fn block(&self, chat: &ChatId, blocked: bool, cx: &App) -> Task<Result<(), ActionError>> {
        let jid = match jid(chat) {
            Ok(jid) => jid,
            Err(error) => return Task::ready(Err(error)),
        };
        self.connection.run(cx, move |client| async move {
            let blocking = client.blocking();
            let result = if blocked {
                blocking.block(&jid).await
            } else {
                blocking.unblock(&jid).await
            };
            result.map_err(|error| ActionError::refused(&error))
        })
    }

    /// Everyone this account blocked, by every address they go by: WhatsApp
    /// lists blocks by LID, while chats may still go by phone number.
    pub fn blocklist(&self, cx: &App) -> Task<Result<Vec<ChatId>, ActionError>> {
        self.connection.run(cx, move |client| async move {
            let entries = client
                .blocking()
                .get_blocklist()
                .await
                .map_err(|error| ActionError::refused(&error))?;
            let mut addresses = Vec::with_capacity(entries.len() * 2);
            for entry in entries {
                let bare = entry.jid.to_non_ad();
                // Without a mapping the block still shows under the address
                // WhatsApp gave.
                if let Ok(Some(mapping)) = client.get_lid_pn_entry(&bare).await {
                    for other in [Jid::lid(&*mapping.lid), Jid::pn(&*mapping.phone_number)] {
                        if other != bare {
                            addresses.push(ChatId::new(&other.to_string()));
                        }
                    }
                }
                addresses.push(ChatId::new(&bare.to_string()));
            }
            Ok(addresses)
        })
    }
}
