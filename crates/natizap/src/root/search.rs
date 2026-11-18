//! Searching every chat from the chat list: what is typed goes to the
//! messages' search, its results show under the chats, and a result opens
//! its message.

use std::collections::HashMap;

use gpui::{AppContext as _, Context, Entity, SharedString};
use natizap_chats::{ChatList, ChatListView, QueryChanged};
use natizap_messages::{ConversationView, MessageLog, MessageSearch, SearchHitOpened};
use natizap_protocol::ChatId;

use super::Root;

impl Root {
    /// Hands the chat list the messages' search, keeps it told of what is
    /// typed and what chats are called, and opens a result.
    pub(super) fn search_messages(
        chat_list: &Entity<ChatListView>,
        chats: &Entity<ChatList>,
        messages: &Entity<MessageLog>,
        conversation: &Entity<ConversationView>,
        cx: &mut Context<Self>,
    ) {
        let search = cx.new(|_| MessageSearch::new(messages.clone()));
        chat_list.update(cx, |list, cx| {
            list.set_message_results(search.clone().into(), cx);
        });

        let typed = search.clone();
        cx.subscribe(chat_list, move |_, _, QueryChanged(query), cx| {
            typed.update(cx, |search, cx| search.set_query(query, cx));
        })
        .detach();

        let naming = search.clone();
        cx.observe(chats, move |_, chats, cx| {
            let chats = chats.read(cx);
            let titles: HashMap<ChatId, SharedString> = chats
                .rows()
                .iter()
                .chain(chats.archived().iter())
                .flat_map(|row| {
                    row.addresses
                        .iter()
                        .chain([&row.id])
                        .map(|address| (address.clone(), row.title.clone()))
                })
                .collect();
            naming.update(cx, |search, cx| search.set_titles(titles, cx));
        })
        .detach();

        let (rows, conversation) = (chats.clone(), conversation.clone());
        cx.subscribe(&search, move |_, _, hit: &SearchHitOpened, cx| {
            let addresses = rows
                .read(cx)
                .rows()
                .iter()
                .chain(rows.read(cx).archived().iter())
                .find(|row| row.id == hit.chat || row.addresses.contains(&hit.chat))
                .map_or_else(|| vec![hit.chat.clone()], |row| row.addresses.to_vec());
            conversation.update(cx, |conversation, cx| {
                conversation.go_to(hit.id.clone(), hit.sent_at, addresses, cx);
            });
        })
        .detach();
    }
}
