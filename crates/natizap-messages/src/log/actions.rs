//! What the user does to messages already in a chat: react, delete, star
//! and pin. Each applies locally at once and is sent on in the background.

use chrono::Local;
use gpui::{AppContext as _, Context};
use natizap_protocol::{ChatId, PinFor};

use super::{Input, MessageLog};
use crate::message::MessageChange;

impl MessageLog {
    /// Deletes this account's own message in the open chat for everyone.
    pub(crate) fn delete_for_everyone(&mut self, message_id: String, cx: &mut Context<Self>) {
        let Some(chat) = self.open.as_ref().map(|open| open.id.clone()) else {
            return;
        };
        let deleting = self
            .protocol
            .read(cx)
            .delete_for_everyone(&chat, message_id.clone(), cx);
        self.send_input(Input::from(MessageChange::Deleted {
            chat,
            id: message_id,
        }));
        cx.background_spawn(async move {
            if let Err(error) = deleting.await {
                tracing::warn!(%error, "message not deleted for everyone");
            }
        })
        .detach();
    }

    /// Deletes a message in the open chat from this account's devices.
    /// `sender` wrote it, in groups, unless `from_me`.
    pub(crate) fn delete_for_me(
        &mut self,
        message_id: String,
        from_me: bool,
        sender: Option<&ChatId>,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.open.as_ref().map(|open| open.id.clone()) else {
            return;
        };
        let deleting =
            self.protocol
                .read(cx)
                .delete_for_me(&chat, message_id.clone(), from_me, sender, cx);
        self.send_input(Input::from(MessageChange::Removed {
            chat,
            id: message_id,
        }));
        cx.background_spawn(async move {
            if let Err(error) = deleting.await {
                tracing::warn!(%error, "message not deleted from the other devices");
            }
        })
        .detach();
    }

    /// Pins a message in the open chat for everyone for `pin`, or unpins it
    /// when `pin` is `None`. `sender` wrote it, in groups, unless `from_me`.
    pub(crate) fn set_pinned(
        &mut self,
        message_id: String,
        from_me: bool,
        sender: Option<&ChatId>,
        pin: Option<PinFor>,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.open.as_ref().map(|open| open.id.clone()) else {
            return;
        };
        let pinning =
            self.protocol
                .read(cx)
                .pin(&chat, message_id.clone(), from_me, sender, pin, cx);
        let until = pin.map(|pin| Local::now().timestamp() + pin.seconds());
        self.send_input(Input::from(MessageChange::Pinned { message_id, until }));
        cx.background_spawn(async move {
            if let Err(error) = pinning.await {
                tracing::warn!(%error, "pin not sent");
            }
        })
        .detach();
    }

    /// Stars or unstars a message in the open chat on all of this account's
    /// devices. `sender` wrote it, in groups, unless `from_me`.
    pub(crate) fn set_starred(
        &mut self,
        message_id: String,
        from_me: bool,
        sender: Option<&ChatId>,
        starred: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.open.as_ref().map(|open| open.id.clone()) else {
            return;
        };
        let starring =
            self.protocol
                .read(cx)
                .star(&chat, message_id.clone(), from_me, sender, starred, cx);
        self.send_input(Input::from(MessageChange::Starred {
            message_id,
            starred,
        }));
        cx.background_spawn(async move {
            if let Err(error) = starring.await {
                tracing::warn!(%error, "star not synced to the other devices");
            }
        })
        .detach();
    }

    /// Reacts to a message in the open chat with `emoji`, or takes this
    /// account's reaction back when `emoji` is `None`.
    pub(crate) fn react(
        &mut self,
        message_id: String,
        sender: Option<ChatId>,
        emoji: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.open.as_ref().map(|open| open.id.clone()) else {
            return;
        };
        self.send_input(Input::from(MessageChange::Reacted {
            chat: chat.clone(),
            message_id: message_id.clone(),
            reactor: None,
            emoji: emoji.clone(),
        }));
        let reacting = self
            .protocol
            .read(cx)
            .react(&chat, message_id, sender, emoji, cx);
        cx.background_spawn(async move {
            if let Err(error) = reacting.await {
                tracing::warn!(%error, "reaction not sent");
            }
        })
        .detach();
    }
}
