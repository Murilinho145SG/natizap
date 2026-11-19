//! The Status place and what it opens: the list in place of the chats,
//! the viewer over the window, the composer, replies sent to the poster's
//! chat, and the rings the chat list draws around people with a status.

use std::collections::HashMap;

use gpui::{AppContext as _, Context, Entity, SharedString, Window};
use natizap_chats::{ChatList, ChatListView, StatusRequested};
use natizap_messages::MessageLog;
use natizap_player::PlayerView;
use natizap_protocol::ChatId;
use natizap_status::{
    ComposerClosed, PostRequested, ReplyBody, StatusBook, StatusClosed, StatusComposer,
    StatusOpened, StatusPane, StatusPlayRequested, StatusReply, StatusViewer,
};

use super::Root;

impl Root {
    /// Keeps the status book told who people are and who sees this
    /// account's posts, the chat list told who has a status, and opens a
    /// status from a ringed avatar.
    pub(super) fn follow_status(
        status: &Entity<StatusBook>,
        chats: &Entity<ChatList>,
        chat_list: &Entity<ChatListView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let book = status.clone();
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
            // "My contacts": the people saved in the address book, by their
            // phone numbers, which status updates are sent to.
            let audience: Vec<ChatId> = chats
                .names()
                .iter()
                .filter(|(id, name)| name.saved.is_some() && id.phone_number().is_some())
                .map(|(id, _)| id.clone())
                .collect();
            book.update(cx, |book, cx| {
                book.set_titles(titles, cx);
                book.set_audience(audience);
            });
        })
        .detach();

        let list = chat_list.clone();
        cx.observe(status, move |_, status, cx| {
            let rings = status.read(cx).rings();
            list.update(cx, |list, cx| list.set_status_rings(rings, cx));
        })
        .detach();

        cx.subscribe_in(
            chat_list,
            window,
            |root, _, StatusRequested(chat), window, cx| {
                root.open_status(chat, window, cx);
            },
        )
        .detach();
    }

    /// Shows the Status place in place of the chat list.
    pub(super) fn show_status_place(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.status_pane.is_none() {
            let me = self.protocol.read(cx).me().map(|me| {
                let name = me.name.clone().unwrap_or_default();
                (me.phone, SharedString::from(name))
            });
            let (book, pictures) = (self.status.clone(), self.pictures.clone());
            let pane = cx.new(|cx| StatusPane::new(book, pictures, me, cx));
            cx.subscribe_in(&pane, window, |root, _, StatusOpened(chat), window, cx| {
                root.open_status(chat, window, cx);
            })
            .detach();
            cx.subscribe_in(&pane, window, |root, _, _: &PostRequested, window, cx| {
                root.compose_status(window, cx);
            })
            .detach();
            self.status_pane = Some(pane);
        }
        self.own_shown = false;
        self.status_shown = true;
        cx.notify();
    }

    /// Plays `chat`'s status over the window.
    pub(super) fn open_status(
        &mut self,
        chat: &ChatId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (book, pictures) = (self.status.clone(), self.pictures.clone());
        let viewer = cx.new(|cx| StatusViewer::new(book, pictures, chat, window, cx));
        cx.subscribe(&viewer, |root, _, _: &StatusClosed, cx| {
            root.status_viewer = None;
            cx.notify();
        })
        .detach();
        cx.subscribe_in(
            &viewer,
            window,
            |_, viewer, play: &StatusPlayRequested, window, cx| {
                let player = cx.new(|cx| PlayerView::new(&play.file, window, cx));
                viewer.update(cx, |viewer, cx| {
                    viewer.show_player(&play.id, player.into(), cx);
                });
            },
        )
        .detach();
        let messages = self.messages.clone();
        cx.subscribe(&viewer, move |root, _, reply: &StatusReply, cx| {
            root.send_status_reply(&messages, reply, cx);
        })
        .detach();
        self.status_viewer = Some(viewer);
        cx.notify();
    }

    /// Opens the composer that posts a status.
    pub(super) fn compose_status(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let book = self.status.clone();
        let composer = cx.new(|cx| StatusComposer::new(book, window, cx));
        cx.subscribe(&composer, |root, _, _: &ComposerClosed, cx| {
            root.status_composer = None;
            cx.notify();
        })
        .detach();
        self.status_composer = Some(composer);
        cx.notify();
    }

    /// Sends a reply to a status to its poster's chat, opening that chat
    /// so it goes there.
    fn send_status_reply(
        &mut self,
        messages: &Entity<MessageLog>,
        reply: &StatusReply,
        cx: &mut Context<Self>,
    ) {
        let title = self.status.read(cx).title(&reply.poster);
        self.chat_with(&reply.poster, title, cx);
        let quoted = Some(reply.reply.clone());
        messages.update(cx, |messages, cx| match &reply.body {
            ReplyBody::Text(text) => messages.send(text.clone(), quoted, cx),
            ReplyBody::Voice(heard) => messages.send_voice(heard.clone(), quoted, cx),
        });
    }
}
