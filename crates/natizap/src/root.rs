use gpui::prelude::*;
use gpui::{Context, Entity, Window, div, px, rgb};
use natizap_chats::{ChatList, ChatListView};
use natizap_session::{PairingView, Session, SessionState};

/// The main window's content: the pairing screen until this device is linked,
/// then the chat list beside the conversation pane.
pub(crate) struct Root {
    session: Entity<Session>,
    chats: Entity<ChatList>,
    pairing: Entity<PairingView>,
    chat_list: Entity<ChatListView>,
}

impl Root {
    pub(crate) fn new(
        session: Entity<Session>,
        chats: Entity<ChatList>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&session, |_, _, cx| cx.notify()).detach();
        cx.observe(&chats, |_, _, cx| cx.notify()).detach();
        let pairing = cx.new(|cx| PairingView::new(session.clone(), cx));
        let chat_list = cx.new(|cx| ChatListView::new(chats.clone(), cx));
        Self {
            session,
            chats,
            pairing,
            chat_list,
        }
    }

    /// Whether to show the chats. While connecting, cached chats mean this
    /// device was linked before, so they show right away instead of waiting
    /// for the connection.
    fn shows_chats(&self, cx: &Context<Self>) -> bool {
        let session = self.session.read(cx);
        session.is_linked()
            || matches!(session.state(), SessionState::Starting)
                && !self.chats.read(cx).rows().is_empty()
    }
}

impl Render for Root {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.shows_chats(cx) {
            return div().size_full().child(self.pairing.clone());
        }
        let status = match self.session.read(cx).state() {
            SessionState::Starting | SessionState::Reconnecting => Some("Connecting…"),
            SessionState::Paired => Some("Syncing chats from your phone…"),
            _ => None,
        };
        div()
            .size_full()
            .flex()
            .child(
                div()
                    .w(px(400.0))
                    .h_full()
                    .flex()
                    .flex_col()
                    .border_r_1()
                    .border_color(rgb(0x22_2d_34))
                    .children(status.map(|status| {
                        div()
                            .px_4()
                            .py_2()
                            .text_sm()
                            .bg(rgb(0x18_2a_33))
                            .text_color(rgb(0x86_96_a0))
                            .child(status)
                    }))
                    .child(div().flex_1().min_h_0().child(self.chat_list.clone())),
            )
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(rgb(0x22_2e_35))
                    .text_color(rgb(0x86_96_a0))
                    .child("Select a chat to start messaging"),
            )
    }
}
