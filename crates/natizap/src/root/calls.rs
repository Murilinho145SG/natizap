//! The Calls place: the call history in place of the chat list, a call
//! opening its chat.

use gpui::{AppContext as _, Context, Entity, Window};
use natizap_calls::{CallChatRequested, CallLog, CallsPane};
use natizap_chats::ChatList;

use super::{Place, Root};

impl Root {
    /// Keeps the call history told what chats are called.
    pub(super) fn follow_calls(
        calls: &Entity<CallLog>,
        chats: &Entity<ChatList>,
        cx: &mut Context<Self>,
    ) {
        let log = calls.clone();
        cx.observe(chats, move |_, chats, cx| {
            let titles = chats.read(cx).titles();
            log.update(cx, |log, cx| log.set_titles(titles, cx));
        })
        .detach();
    }

    /// Shows the Calls place in place of the chat list.
    pub(super) fn show_calls_place(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.calls_pane.is_none() {
            let (log, pictures) = (self.calls.clone(), self.pictures.clone());
            let pane = cx.new(|cx| CallsPane::new(log, pictures, cx));
            cx.subscribe_in(&pane, window, |root, _, CallChatRequested(chat), _, cx| {
                let title = root.calls.read(cx).title(chat);
                root.place = Place::Chats;
                root.chat_with(chat, title, cx);
            })
            .detach();
            self.calls_pane = Some(pane);
        }
        self.own_shown = false;
        self.place = Place::Calls;
        cx.notify();
    }
}
