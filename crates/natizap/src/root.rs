use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, Entity, FontWeight, Window, div, px, rgb, svg};
use natizap_chats::{ChatList, ChatListView};
use natizap_session::{PairingView, Session, SessionState};
use natizap_ui::theme::{ALERT, DISPLAY_FONT, HONEY, LINE, MINT, MUTED, PANE, TEXT, UI_FONT};
use natizap_ui::{Icon, window_frame};

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

    fn content(&self, cx: &Context<Self>) -> AnyElement {
        if !self.shows_chats(cx) {
            return self.pairing.clone().into_any_element();
        }
        div()
            .size_full()
            .flex()
            .child(
                div()
                    .w(px(380.0))
                    .h_full()
                    .flex_none()
                    .border_r_1()
                    .border_color(rgb(LINE))
                    .child(self.chat_list.clone()),
            )
            .child(conversation_placeholder())
            .into_any_element()
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = wordmark(self.session.read(cx).state());
        let content = self.content(cx);
        div()
            .size_full()
            .font_family(UI_FONT)
            .text_color(rgb(TEXT))
            .child(window_frame(window, title, content))
    }
}

/// The app name with a dot that tells, at a glance, whether natizap is
/// connected; anything but connected is spelled out next to it.
fn wordmark(state: &SessionState) -> Div {
    let (dot, label) = match state {
        SessionState::Connected => (MINT, None),
        SessionState::Starting | SessionState::Reconnecting => (HONEY, Some("Connecting…")),
        SessionState::Paired => (HONEY, Some("Syncing chats…")),
        SessionState::AwaitingPairing(_) | SessionState::PairingExpired => {
            (LINE, Some("Not linked"))
        }
        SessionState::LoggedOut => (ALERT, Some("Unlinked")),
        SessionState::Failed(_) => (ALERT, Some("Offline")),
    };
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .font_family(DISPLAY_FONT)
                .font_weight(FontWeight::BOLD)
                .text_size(px(17.0))
                .child("natizap"),
        )
        .child(div().size(px(7.0)).rounded_full().bg(rgb(dot)))
        .children(label.map(|label| {
            div()
                .text_size(px(12.5))
                .text_color(rgb(MUTED))
                .child(label)
        }))
}

fn conversation_placeholder() -> Div {
    div()
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_3()
        .bg(rgb(PANE))
        .child(
            svg()
                .path(Icon::Chats.path())
                .size(px(56.0))
                .text_color(rgb(LINE)),
        )
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .child("Pick a chat to read it here"),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_1p5()
                .text_size(px(12.5))
                .text_color(rgb(MUTED))
                .child(
                    svg()
                        .path(Icon::Lock.path())
                        .size(px(13.0))
                        .text_color(rgb(MUTED)),
                )
                .child("End-to-end encrypted"),
        )
}
