use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, Entity, FontWeight, Window, div, px, rgb};
use natizap_chats::{ChatList, ChatListView, ChatSelected};
use natizap_messages::{ConversationView, MessageLog, MessageSent, OpenChat, PlayRequested};
use natizap_player::{PlayerClosed, PlayerView};
use natizap_session::{PairingView, Session, SessionState};
use natizap_ui::theme::{ACCENT, ALERT, DISPLAY_FONT, LINE, MINT, MUTED, TEXT, base_font};
use natizap_ui::{Alert, AlertTone, window_frame};

/// The main window's content: the pairing screen until this device is linked,
/// then the chat list beside the conversation pane.
pub(crate) struct Root {
    session: Entity<Session>,
    chats: Entity<ChatList>,
    pairing: Entity<PairingView>,
    chat_list: Entity<ChatListView>,
    conversation: Entity<ConversationView>,
    /// The video or audio playing over the chats, if any.
    player: Option<Entity<PlayerView>>,
}

impl Root {
    pub(crate) fn new(
        session: Entity<Session>,
        chats: Entity<ChatList>,
        messages: &Entity<MessageLog>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&chats, |_, _, cx| cx.notify()).detach();
        let pairing = cx.new(|cx| PairingView::new(session.clone(), cx));
        let chat_list = cx.new(|cx| ChatListView::new(chats.clone(), cx));
        let syncing_list = chat_list.clone();
        cx.observe(&session, move |_, session, cx| {
            // Right after linking the list fills from the first sync.
            let syncing = matches!(session.read(cx).state(), SessionState::Paired);
            syncing_list.update(cx, |list, cx| list.set_syncing(syncing, cx));
            cx.notify();
        })
        .detach();
        let conversation = cx.new(|cx| ConversationView::new(messages.clone(), window, cx));

        let (open_messages, open_chats) = (messages.clone(), chats.clone());
        cx.subscribe(&chat_list, move |_, _, ChatSelected(row), cx| {
            open_chats.read(cx).open(Some(row.id.clone()));
            let chat = OpenChat {
                id: row.id.clone(),
                addresses: row.addresses.to_vec(),
                title: row.title.clone(),
                unread: row.unread,
            };
            open_messages.update(cx, |messages, cx| messages.open(chat, cx));
        })
        .detach();
        let sent_chats = chats.clone();
        cx.subscribe(messages, move |_, _, sent: &MessageSent, cx| {
            sent_chats
                .read(cx)
                .record_sent(sent.chat.clone(), sent.text.clone(), sent.sent_at);
        })
        .detach();
        cx.subscribe_in(
            messages,
            window,
            |root, _, play: &PlayRequested, window, cx| {
                let player =
                    cx.new(|cx| PlayerView::new(&play.file, play.title.clone(), window, cx));
                cx.subscribe(&player, |root, _, _: &PlayerClosed, cx| {
                    root.player = None;
                    cx.notify();
                })
                .detach();
                root.player = Some(player);
                cx.notify();
            },
        )
        .detach();

        Self {
            session,
            chats,
            pairing,
            chat_list,
            conversation,
            player: None,
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
        if let Some(player) = &self.player {
            return player.clone().into_any_element();
        }
        let reconnecting = matches!(self.session.read(cx).state(), SessionState::Reconnecting);
        div()
            .size_full()
            .relative()
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
            .child(div().flex_1().h_full().child(self.conversation.clone()))
            // Chats stay readable offline, so the news floats over them
            // instead of replacing them.
            .when(reconnecting, |content| {
                content.child(
                    div()
                        .absolute()
                        .top_3()
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .child(
                            Alert::new(AlertTone::Warning, "Reconnecting…")
                                .description("The connection dropped. natizap keeps trying."),
                        ),
                )
            })
            .into_any_element()
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = wordmark(self.session.read(cx).state());
        let content = self.content(cx);
        div()
            .size_full()
            .font(base_font())
            .text_color(rgb(TEXT))
            .child(window_frame(window, title, content))
    }
}

/// The app name with a dot that tells, at a glance, whether natizap is
/// connected; anything but connected is spelled out next to it.
fn wordmark(state: &SessionState) -> Div {
    let (dot, label) = match state {
        SessionState::Connected => (MINT, None),
        SessionState::Starting | SessionState::Reconnecting => (ACCENT, Some("Connecting…")),
        SessionState::Paired => (ACCENT, Some("Syncing chats…")),
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
