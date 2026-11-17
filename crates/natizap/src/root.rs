use gpui::prelude::*;
use gpui::{
    AnyElement, ClipboardItem, Context, Div, Entity, FontWeight, MouseButton, NavigationDirection,
    Window, div, px, rgb,
};
use natizap_chats::{
    ChatList, ChatListView, ChatPicker, ChatRow, ChatSelected, ChatsPicked, PickerDismissed,
};
use natizap_contacts::{
    ChatSettings, ChatSummary, DisappearingChosen, MuteToggled, ProfileClosed, ProfilePanel, Tile,
};
use natizap_messages::{
    ChatRequested, ContactsRequested, ConversationView, ForwardRequested, MessageLog, MessageSent,
    OpenChat, PlayRequested, ProfileRequested,
};
use natizap_player::{PlayerClosed, PlayerView};
use natizap_profiles::ProfilePictures;
use natizap_protocol::{Card, ChatId, Extra, Protocol};
use natizap_session::{PairingView, Session, SessionState};
use natizap_ui::theme::{ACCENT, ALERT, DISPLAY_FONT, LINE, MINT, MUTED, TEXT, base_font};
use natizap_ui::{Alert, AlertTone, window_frame};

use crate::history::History;

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
    /// Choosing chats to forward a message to.
    picker: Option<Entity<ChatPicker>>,
    /// The open chat's profile, beside the conversation.
    profile: Option<Entity<ProfilePanel>>,
    messages: Entity<MessageLog>,
    /// Chats visited, for the mouse's back and forward buttons.
    history: History<ChatId>,
}

impl Root {
    pub(crate) fn new(
        session: Entity<Session>,
        chats: Entity<ChatList>,
        messages: &Entity<MessageLog>,
        pictures: &Entity<ProfilePictures>,
        protocol: Entity<Protocol>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&chats, |_, _, cx| cx.notify()).detach();
        let pairing = cx.new(|cx| PairingView::new(session.clone(), cx));
        let chat_list = cx.new(|cx| ChatListView::new(chats.clone(), pictures.clone(), cx));
        let syncing_list = chat_list.clone();
        cx.observe(&session, move |_, session, cx| {
            // Right after linking the list fills from the first sync.
            let syncing = matches!(session.read(cx).state(), SessionState::Paired);
            syncing_list.update(cx, |list, cx| list.set_syncing(syncing, cx));
            cx.notify();
        })
        .detach();
        let conversation =
            cx.new(|cx| ConversationView::new(messages.clone(), pictures.clone(), window, cx));

        cx.subscribe(&chat_list, |root, _, ChatSelected(row), cx| {
            root.history.visit(row.id.clone());
            root.open_chat(row, cx);
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

        Self::forward_through_picker(messages, &chats, pictures, window, cx);
        Self::open_profiles(&conversation, protocol, cx);
        Self::open_numbers(&conversation, cx);
        Self::share_contacts(&conversation, messages, &chats, pictures, window, cx);

        Self {
            session,
            chats,
            pairing,
            chat_list,
            conversation,
            player: None,
            picker: None,
            profile: None,
            messages: messages.clone(),
            history: History::default(),
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

    /// Answers the conversation's forward requests with the chat picker,
    /// handing the chosen chats back to the message log.
    fn forward_through_picker(
        messages: &Entity<MessageLog>,
        chats: &Entity<ChatList>,
        pictures: &Entity<ProfilePictures>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (forward_chats, forward_pictures) = (chats.clone(), pictures.clone());
        let forward_messages = messages.clone();
        cx.subscribe_in(
            messages,
            window,
            move |root, _, request: &ForwardRequested, window, cx| {
                let picker = cx.new(|cx| {
                    ChatPicker::new(
                        "Forward to",
                        forward_chats.clone(),
                        forward_pictures.clone(),
                        window,
                        cx,
                    )
                });
                let (messages, request) = (forward_messages.clone(), request.clone());
                cx.subscribe(&picker, move |root, _, ChatsPicked(chats), cx| {
                    messages.update(cx, |log, cx| log.forward(&request, chats.clone(), cx));
                    root.picker = None;
                    cx.notify();
                })
                .detach();
                cx.subscribe(&picker, |root, _, _: &PickerDismissed, cx| {
                    root.picker = None;
                    cx.notify();
                })
                .detach();
                root.picker = Some(picker);
                cx.notify();
            },
        )
        .detach();
    }

    /// Answers the conversation's request to share contacts with the chat
    /// picker, sending the people chosen as contact cards.
    fn share_contacts(
        conversation: &Entity<ConversationView>,
        messages: &Entity<MessageLog>,
        chats: &Entity<ChatList>,
        pictures: &Entity<ProfilePictures>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (chats, pictures, messages) = (chats.clone(), pictures.clone(), messages.clone());
        cx.subscribe_in(
            conversation,
            window,
            move |root, _, _: &ContactsRequested, window, cx| {
                let picker = cx.new(|cx| {
                    ChatPicker::new(
                        "Share contacts",
                        chats.clone(),
                        pictures.clone(),
                        window,
                        cx,
                    )
                });
                let (chats, messages) = (chats.clone(), messages.clone());
                cx.subscribe(&picker, move |root, _, ChatsPicked(picked), cx| {
                    let cards: Vec<Card> = chats
                        .read(cx)
                        .rows()
                        .iter()
                        .filter(|row| picked.contains(&row.id))
                        .filter_map(|row| {
                            // Groups have no number to share.
                            let phone = row
                                .addresses
                                .iter()
                                .find(|address| address.phone_number().is_some())?;
                            Some(Card {
                                name: row.title.to_string(),
                                phone: phone.as_str().split('@').next().map(str::to_owned),
                            })
                        })
                        .collect();
                    if !cards.is_empty() {
                        messages.update(cx, |log, cx| log.send_extra(Extra::Cards(cards), cx));
                    }
                    root.picker = None;
                    cx.notify();
                })
                .detach();
                cx.subscribe(&picker, |root, _, _: &PickerDismissed, cx| {
                    root.picker = None;
                    cx.notify();
                })
                .detach();
                root.picker = Some(picker);
                cx.notify();
            },
        )
        .detach();
    }

    /// Opens the chat with a number clicked in a message. Without one, copies
    /// the number, since a chat cannot open before it has a message.
    fn open_numbers(conversation: &Entity<ConversationView>, cx: &mut Context<Self>) {
        cx.subscribe(conversation, |root, _, ChatRequested { phone }, cx| {
            let row = root
                .chats
                .read(cx)
                .rows()
                .iter()
                .find(|row| row.addresses.contains(phone))
                .cloned();
            if let Some(row) = row {
                root.history.visit(row.id.clone());
                root.open_chat(&row, cx);
            } else {
                let number = phone
                    .phone_number()
                    .unwrap_or_else(|| phone.as_str().to_owned());
                cx.write_to_clipboard(ClipboardItem::new_string(number));
            }
        })
        .detach();
    }

    /// Shows a chat's profile beside the conversation when its header is
    /// clicked.
    fn open_profiles(
        conversation: &Entity<ConversationView>,
        protocol: Entity<Protocol>,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe(
            conversation,
            move |root, _, request: &ProfileRequested, cx| {
                let panel = cx.new(|cx| {
                    ProfilePanel::new(
                        &protocol,
                        request.chat.clone(),
                        request.title.clone(),
                        request.verified,
                        cx,
                    )
                });
                cx.subscribe(&panel, |root, _, _: &ProfileClosed, cx| {
                    root.profile = None;
                    cx.notify();
                })
                .detach();
                root.fill_profile(&panel, &request.chat, cx);
                root.profile = Some(panel);
                cx.notify();
            },
        )
        .detach();
    }

    /// Gives the profile of `chat` what the other features know of it: its
    /// media and starred messages, and its notifications and disappearing
    /// messages, which it can change.
    fn fill_profile(&self, panel: &Entity<ProfilePanel>, chat: &ChatId, cx: &mut Context<Self>) {
        let Some(row) = self
            .chats
            .read(cx)
            .rows()
            .iter()
            .find(|row| row.addresses.contains(chat))
            .cloned()
        else {
            return;
        };
        let settings = ChatSettings {
            muted: row.muted,
            disappearing: row.disappearing,
        };
        panel.update(cx, |panel, cx| panel.set_settings(settings, cx));

        let reading = self.messages.read(cx).overview(row.addresses.to_vec(), cx);
        let summarized = panel.downgrade();
        cx.spawn(async move |_, cx| {
            let overview = reading.await;
            let summary = ChatSummary {
                media: overview.media,
                tiles: overview
                    .recent
                    .into_iter()
                    .map(|tile| Tile {
                        image: tile.image.map(Into::into),
                        length: tile.length,
                    })
                    .collect(),
                starred: overview.starred,
            };
            let _ = summarized.update(cx, |panel, cx| panel.set_summary(summary, cx));
        })
        .detach();

        let (chats, id) = (self.chats.clone(), row.id.clone());
        cx.subscribe(panel, move |_, _, MuteToggled(muted), cx| {
            chats.read(cx).set_muted(&id, *muted, cx);
        })
        .detach();
        let (chats, id) = (self.chats.clone(), row.id.clone());
        cx.subscribe(panel, move |_, _, DisappearingChosen(timer), cx| {
            chats.read(cx).set_disappearing(&id, *timer, cx);
        })
        .detach();
    }

    /// Shows `row`'s chat in the list and the conversation.
    fn open_chat(&mut self, row: &ChatRow, cx: &mut Context<Self>) {
        // The profile beside the conversation was of the chat left behind.
        self.profile = None;
        self.chats.read(cx).open(Some(row.id.clone()));
        let chat = OpenChat {
            id: row.id.clone(),
            verified: row.verified,
            addresses: row.addresses.to_vec(),
            title: row.title.clone(),
            unread: row.unread,
        };
        self.messages
            .update(cx, |messages, cx| messages.open(chat, cx));
        self.chat_list.update(cx, |list, cx| list.show(&row.id, cx));
    }

    /// Like the phone's back button: closes what is on top first (the
    /// player, the chat picker, a menu or the reply being composed), then
    /// goes back to the chat before.
    fn go_back(&mut self, cx: &mut Context<Self>) {
        if self.player.take().is_some()
            || self.picker.take().is_some()
            || self.profile.take().is_some()
        {
            cx.notify();
            return;
        }
        if self.conversation.update(cx, ConversationView::step_back) {
            return;
        }
        let Some(chat) = self.history.back().cloned() else {
            return;
        };
        self.show_visited(&chat, cx);
    }

    /// Returns to the chat last gone back from.
    fn go_forward(&mut self, cx: &mut Context<Self>) {
        let Some(chat) = self.history.forward().cloned() else {
            return;
        };
        self.show_visited(&chat, cx);
    }

    /// Opens a chat from the history; one gone from the list is forgotten.
    fn show_visited(&mut self, chat: &ChatId, cx: &mut Context<Self>) {
        let row = self
            .chats
            .read(cx)
            .rows()
            .iter()
            .find(|row| row.id == *chat)
            .cloned();
        match row {
            Some(row) => self.open_chat(&row, cx),
            None => self.history.forget(chat),
        }
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
            .children(
                self.profile
                    .clone()
                    .map(|profile| div().w(px(380.0)).h_full().flex_none().child(profile)),
            )
            .children(self.picker.clone())
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
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(|root, _, _, cx| root.go_back(cx)),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                cx.listener(|root, _, _, cx| root.go_forward(cx)),
            )
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
