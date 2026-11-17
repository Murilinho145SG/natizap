//! Tells the notifier what the user is looking at and which chats are
//! muted, so it chimes only for messages elsewhere, and opens its settings.

use std::collections::HashSet;

use gpui::prelude::*;
use gpui::{Context, Entity, Window};
use natizap_chats::{ChatList, ChatListView, SettingsRequested};
use natizap_messages::MessageLog;
use natizap_notifications::{NotificationSettings, Notifier, SettingsClosed};
use natizap_protocol::ChatId;

use super::Root;

impl Root {
    /// Keeps `notifier` told of the window's focus, the open chat and the
    /// muted chats.
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

        let muted = notifier.clone();
        cx.observe(chats, move |_, chats, cx| {
            let chats = chats.read(cx);
            let addresses: HashSet<ChatId> = chats
                .rows()
                .iter()
                .chain(chats.archived().iter())
                .filter(|row| row.muted)
                .flat_map(|row| row.addresses.iter().chain([&row.id]).cloned())
                .collect();
            muted.update(cx, |notifier, _| notifier.set_muted(addresses));
        })
        .detach();
    }

    /// Shows the notification settings when the chat list asks for them.
    pub(super) fn open_settings(
        chat_list: &Entity<ChatListView>,
        notifier: &Entity<Notifier>,
        cx: &mut Context<Self>,
    ) {
        let notifier = notifier.clone();
        cx.subscribe(chat_list, move |root, _, _: &SettingsRequested, cx| {
            let settings = cx.new(|cx| NotificationSettings::new(notifier.clone(), cx));
            cx.subscribe(&settings, |root, _, _: &SettingsClosed, cx| {
                root.settings = None;
                cx.notify();
            })
            .detach();
            root.settings = Some(settings);
            cx.notify();
        })
        .detach();
    }
}
