//! Tells the notifier what the user is looking at, which chats are muted
//! and what chats and people are called, and opens the chat a notification
//! is about once the shell has shown the window.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui::{Context, Entity, SharedString, Window};
use natizap_chats::{ChatList, ChatRow};
use natizap_messages::MessageLog;
use natizap_notifications::Notifier;
use natizap_protocol::ChatId;

use super::Root;

impl Root {
    /// Keeps `notifier` told of the window's focus, the open chat, the muted
    /// chats and everyone's names.
    pub(super) fn feed_notifier(
        notifier: &Entity<Notifier>,
        chats: &Entity<ChatList>,
        messages: &Entity<MessageLog>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focused = window.is_window_active();
        notifier.update(cx, |notifier, _| notifier.set_focused(focused));
        let focus = notifier.clone();
        cx.observe_window_activation(window, move |_, window, cx| {
            let focused = window.is_window_active();
            focus.update(cx, |notifier, _| notifier.set_focused(focused));
        })
        .detach();

        let open = notifier.clone();
        cx.observe(messages, move |_, messages, cx| {
            let addresses: HashSet<ChatId> = messages
                .read(cx)
                .open_chat()
                .into_iter()
                .flat_map(|chat| chat.addresses.iter().chain([&chat.id]))
                .cloned()
                .collect();
            open.update(cx, |notifier, _| notifier.set_open(addresses));
        })
        .detach();

        let directory = notifier.clone();
        cx.observe(chats, move |_, chats, cx| {
            let chats = chats.read(cx);
            let rows = || chats.rows().iter().chain(chats.archived().iter());
            let muted: HashSet<ChatId> =
                rows().filter(|row| row.muted).flat_map(addresses).collect();
            let titles: HashMap<ChatId, SharedString> = rows()
                .flat_map(|row| addresses(row).map(|id| (id, row.title.clone())))
                .collect();
            let names = Arc::clone(chats.names());
            directory.update(cx, |notifier, _| {
                notifier.set_muted(muted);
                notifier.set_titles(titles);
                notifier.set_names(names);
            });
        })
        .detach();
    }

    /// Opens the chat a clicked notification was about.
    pub(crate) fn open_notified(&mut self, chat: &ChatId, cx: &mut Context<Self>) {
        let title = chat
            .phone_number()
            .unwrap_or_else(|| chat.as_str().to_owned());
        self.chat_with(chat, title.into(), cx);
    }
}

/// Every address `row`'s chat is known under.
fn addresses(row: &ChatRow) -> impl Iterator<Item = ChatId> + '_ {
    row.addresses.iter().chain([&row.id]).cloned()
}
