//! What the chat list's right-click menu changes beyond the list: the
//! messages of a chat cleared or deleted, and the open chat's header.

use gpui::{Context, Entity};
use natizap_chats::{ChatForgotten, ChatList, ChatListView};
use natizap_messages::MessageLog;

use super::Root;

impl Root {
    /// Forgets the messages of chats cleared or deleted from the list,
    /// closing a deleted one that is open, and keeps the open chat's
    /// header told whether it is muted, wherever that changed.
    pub(super) fn follow_list_menu(
        chat_list: &Entity<ChatListView>,
        chats: &Entity<ChatList>,
        messages: &Entity<MessageLog>,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe(chat_list, |root, _, forgotten: &ChatForgotten, cx| {
            let open = root.messages.read(cx).open_chat().is_some_and(|open| {
                forgotten
                    .addresses
                    .iter()
                    .any(|address| open.addresses.contains(address))
            });
            root.messages
                .read(cx)
                .forget(&forgotten.addresses, forgotten.deleted);
            if forgotten.deleted && open {
                root.close_chat(cx);
            }
        })
        .detach();

        let messages = messages.clone();
        cx.observe(chats, move |_, chats, cx| {
            let Some(open) = messages.read(cx).open_chat() else {
                return;
            };
            let chats = chats.read(cx);
            let muted = chats
                .rows()
                .iter()
                .chain(chats.archived().iter())
                .find(|row| row.id == open.id)
                .map(|row| row.muted);
            if let Some(muted) = muted.filter(|muted| *muted != open.muted) {
                messages.update(cx, |log, cx| log.set_muted(muted, cx));
            }
        })
        .detach();
    }
}
