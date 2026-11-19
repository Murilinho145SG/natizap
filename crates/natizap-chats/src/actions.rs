//! What the chat list's menu does to a whole chat: pinning, archiving,
//! marking read, clearing, deleting and blocking, shown at once and synced
//! to the account's other devices.

use std::collections::HashSet;

use gpui::{App, AppContext as _, Context, Entity, Task};
use natizap_protocol::whatsapp_rust::prelude::EventKind;
use natizap_protocol::{ChatAction, Protocol};

use crate::book::ChatChange;
use crate::list::{ChatList, ChatRow, Input, unix_now};

/// How many chats WhatsApp lets stay pinned at once.
pub(crate) const MAX_PINNED: usize = 3;

impl ChatList {
    /// Applies `action` to `row`'s chat here at once, and on the account's
    /// other devices. Archiving a pinned chat unpins it, as on the phone.
    pub fn act(&self, row: &ChatRow, action: ChatAction, cx: &App) {
        let id = row.id.clone();
        let change = match action {
            ChatAction::Pin(pinned) => ChatChange::Pinned(id, pinned.then(unix_now)),
            ChatAction::Archive(archived) => ChatChange::Archived(id, archived),
            ChatAction::MarkRead(read) => ChatChange::Read(id, read),
            ChatAction::Clear => ChatChange::Cleared(id),
            ChatAction::Delete => ChatChange::Deleted(id),
        };
        self.send(Input::Change(change));
        if action == ChatAction::Archive(true) && row.pinned {
            self.act(row, ChatAction::Pin(false), cx);
        }
        let acting = self
            .protocol
            .read(cx)
            .chat_action(&row.id, action, row.sent_at, cx);
        cx.background_spawn(async move {
            if let Err(error) = acting.await {
                tracing::warn!(%error, ?action, "chat change not synced to the other devices");
            }
        })
        .detach();
    }

    /// Whether another chat may be pinned: WhatsApp keeps at most
    /// [`MAX_PINNED`].
    #[must_use]
    pub fn can_pin(&self) -> bool {
        self.rows().iter().filter(|row| row.pinned).count() < MAX_PINNED
    }

    /// Whether `row` is a person who can be blocked: not a group, a
    /// channel or this account itself.
    #[must_use]
    pub fn can_block(&self, row: &ChatRow, cx: &App) -> bool {
        let protocol = self.protocol.read(cx);
        !row.id.is_group()
            && row.id.is_conversation()
            && !row.addresses.iter().any(|address| protocol.is_own(address))
    }

    /// Whether the person behind `row` is blocked.
    #[must_use]
    pub fn is_blocked(&self, row: &ChatRow) -> bool {
        row.addresses
            .iter()
            .any(|address| self.blocked.contains(address))
    }

    /// Blocks the person behind `row`, or unblocks them; the list shows it
    /// at once, and follows WhatsApp's answer.
    pub fn set_blocked(&mut self, row: &ChatRow, blocked: bool, cx: &mut Context<Self>) {
        for address in row.addresses.iter() {
            if blocked {
                self.blocked.insert(address.clone());
            } else {
                self.blocked.remove(address);
            }
        }
        cx.notify();
        let blocking = self.protocol.read(cx).block(&row.id, blocked, cx);
        cx.spawn(async move |this, cx| {
            if let Err(error) = blocking.await {
                tracing::warn!(%error, blocked, "block not changed");
            }
            // WhatsApp's list says what actually happened, under every
            // address the person goes by.
            let _ = this.update(cx, Self::refresh_blocklist);
        })
        .detach();
    }

    /// Reads who is blocked each time the connection comes up.
    pub(crate) fn follow_connections(
        protocol: &Entity<Protocol>,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let connects = protocol.read(cx).subscribe(&[EventKind::Connected]);
        cx.spawn(async move |this, cx| {
            while connects.recv().await.is_ok() {
                if this.update(cx, Self::refresh_blocklist).is_err() {
                    break;
                }
            }
        })
    }

    fn refresh_blocklist(&mut self, cx: &mut Context<Self>) {
        let reading = self.protocol.read(cx).blocklist(cx);
        cx.spawn(async move |this, cx| match reading.await {
            Ok(blocked) => {
                let _ = this.update(cx, |list, cx| {
                    list.blocked = blocked.into_iter().collect::<HashSet<_>>();
                    cx.notify();
                });
            }
            Err(error) => tracing::warn!(%error, "could not read who is blocked"),
        })
        .detach();
    }
}
