//! Going to a message: scrolling the chat to it, showing the chat back
//! far enough first if it is old, and lighting it up when it shows.

use std::time::Instant;

use gpui::{App, Context, SharedString};
use natizap_protocol::ChatId;

use super::{ChatRequested, ConversationView};
use crate::items::Item;

/// A message being gone to, from the media, docs and links.
#[derive(Debug)]
pub(super) struct GoingTo {
    id: SharedString,
    sent_at: i64,
    /// Every address of the chat it is in.
    chats: Vec<ChatId>,
    /// Whether the chat was asked to show back to it.
    reached: bool,
}

impl ConversationView {
    /// Scrolls to the message `id`, when it is among those loaded.
    pub(crate) fn go_to_message(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(index) = self.position_of(id, cx) {
            self.reveal(index, id, cx);
        }
    }

    /// Goes to the message `id`, sent at `sent_at` in the chat known by
    /// `chats`: opens that chat if another is open, shows it back far
    /// enough, scrolls to the message and lights it up.
    pub(crate) fn go_to(
        &mut self,
        id: SharedString,
        sent_at: i64,
        chats: Vec<ChatId>,
        cx: &mut Context<Self>,
    ) {
        self.browsing = None;
        self.viewing = None;
        self.carousel = None;
        let open = self.log.read(cx).open_chat().map(|chat| chat.id.clone());
        if !open.is_some_and(|open| chats.contains(&open))
            && let Some(chat) = chats.first()
        {
            cx.emit(ChatRequested {
                phone: chat.clone(),
            });
        }
        self.going_to = Some(GoingTo {
            id,
            sent_at,
            chats,
            reached: false,
        });
        self.keep_going(cx);
    }

    /// Takes a step toward the message being gone to: scrolls to it once it
    /// shows, or has the chat show back to it first.
    pub(super) fn keep_going(&mut self, cx: &mut Context<Self>) {
        let Some(going) = self.going_to.as_ref() else {
            return;
        };
        let open = self.log.read(cx).open_chat().map(|chat| chat.id.clone());
        if !open.is_some_and(|open| going.chats.contains(&open)) {
            return;
        }
        let (id, sent_at, reached) = (going.id.clone(), going.sent_at, going.reached);
        if let Some(index) = self.position_of(&id, cx) {
            self.going_to = None;
            self.reveal(index, &id, cx);
        } else if !reached {
            if let Some(going) = self.going_to.as_mut() {
                going.reached = true;
            }
            self.log.read(cx).reach(sent_at);
        }
    }

    fn position_of(&self, id: &str, cx: &App) -> Option<usize> {
        self.log
            .read(cx)
            .items()
            .iter()
            .position(|item| matches!(item, Item::Bubble(bubble) if bubble.id.as_ref() == id))
    }

    /// Scrolls to the message at `index` and lights it up for a moment.
    fn reveal(&mut self, index: usize, id: &str, cx: &mut Context<Self>) {
        self.messages.scroll_to_reveal_item(index);
        self.flash = Some((SharedString::from(id.to_owned()), Instant::now()));
        cx.notify();
    }
}
