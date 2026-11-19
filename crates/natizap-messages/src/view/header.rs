//! The bar over a conversation: who it is with.

use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{App, ClickEvent, Context, Div, FontWeight, Stateful, Window, div, px, rgb};
use natizap_protocol::ChatId;
use natizap_ui::theme::ActiveTheme as _;
use natizap_ui::{Button, ButtonVariant, Icon, Menu, avatar, verified_seal};

use super::{CallRequested, ConversationView, ProfileRequested};
use crate::log::OpenChat;

/// The chat's picture, name and, for a person, their number. Clicking it
/// asks for the chat's profile.
pub(super) fn header(
    chat: &OpenChat,
    picture: Option<PathBuf>,
    members: Option<String>,
    presence: Option<String>,
    cx: &mut Context<ConversationView>,
) -> Stateful<Div> {
    let theme = *cx.theme();
    let subtitle = if chat.id.is_group() {
        Some(members.unwrap_or_else(|| "Group".to_owned()))
    } else {
        presence.or_else(|| {
            chat.id
                .phone_number()
                .filter(|phone| *phone != chat.title.as_ref())
        })
    };
    div()
        .id("chat-header")
        .cursor_pointer()
        // The whole header opens the profile; lighting it up says so.
        .hover(|header| header.bg(rgb(theme.raised)))
        .on_click(cx.listener(|this, _, _, cx| this.request_profile(cx)))
        .h(px(60.0))
        .flex_none()
        .flex()
        .items_center()
        .gap_3()
        .px_4()
        .bg(rgb(theme.panel))
        .border_b_1()
        .border_color(rgb(theme.line))
        .child(avatar(chat.id.as_str(), &chat.title, picture, px(38.0)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_size(px(16.0))
                        // Medium on purpose: at semibold, symbols such as ♥ render
                        // monochrome even with the emoji font as fallback.
                        .font_weight(FontWeight::MEDIUM)
                        .child(div().min_w_0().truncate().child(chat.title.clone()))
                        .when(chat.verified, |title| {
                            title.child(verified_seal(16.0, theme))
                        }),
                )
                .children(subtitle.map(|subtitle| {
                    div()
                        .truncate()
                        .text_size(px(12.5))
                        .text_color(rgb(theme.muted))
                        .child(subtitle)
                })),
        )
        .child(
            div()
                .ml_auto()
                .flex()
                .items_center()
                .gap_1()
                .when(!chat.id.is_group(), |buttons| {
                    buttons.children(call_buttons(&chat.id, cx))
                })
                .child(
                    Button::new("chat-search")
                        .icon(Icon::Search)
                        .variant(ButtonVariant::Ghost)
                        .round()
                        .tooltip("Search (Ctrl+F)")
                        .on_click(cx.listener(|this, _, window, cx| {
                            // The header behind opens the profile.
                            cx.stop_propagation();
                            this.open_find(window, cx);
                        })),
                )
                .child(
                    Button::new("chat-menu")
                        .icon(Icon::More)
                        .variant(ButtonVariant::Ghost)
                        .round()
                        .tooltip("Menu")
                        .on_click(cx.listener(|this, event: &ClickEvent, _, cx| {
                            // The header behind opens the profile.
                            cx.stop_propagation();
                            this.chat_menu = Some(event.position());
                            cx.notify();
                        })),
                ),
        )
}

/// The buttons that call `peer`, with the camera on and without.
fn call_buttons(peer: &ChatId, cx: &mut Context<ConversationView>) -> [Button; 2] {
    [true, false].map(|video| {
        let peer = peer.clone();
        Button::new(if video {
            "chat-video-call"
        } else {
            "chat-call"
        })
        .icon(if video { Icon::Video } else { Icon::Call })
        .variant(ButtonVariant::Ghost)
        .round()
        .tooltip(if video { "Video call" } else { "Voice call" })
        .on_click(cx.listener(move |_, _, _, cx| {
            // The header behind opens the profile.
            cx.stop_propagation();
            cx.emit(CallRequested {
                peer: peer.clone(),
                video,
            });
        }))
    })
}

/// The user chose to close the open chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChatClosed;

/// The user chose to silence the open chat's notifications (`true`) or let
/// them through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MuteRequested(pub bool);

impl ConversationView {
    /// The header's menu, when open: the chat's info, its notifications,
    /// and closing it.
    pub(super) fn chat_menu(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let position = self.chat_menu?;
        let chat = self.log.read(cx).open_chat()?;
        let (muted, group) = (chat.muted, chat.id.is_group());
        let view = cx.weak_entity();
        let act = move |action: fn(&mut ConversationView, &mut Context<ConversationView>)| {
            let view = view.clone();
            move |_: &mut Window, cx: &mut App| {
                let _ = view.update(cx, |this, cx| {
                    this.chat_menu = None;
                    action(this, cx);
                    cx.notify();
                });
            }
        };
        Some(
            Menu::new()
                .action(
                    Icon::Contact,
                    if group { "Group info" } else { "Contact info" },
                    act(ConversationView::request_profile),
                )
                .action(
                    Icon::Muted,
                    if muted {
                        "Unmute notifications"
                    } else {
                        "Mute notifications"
                    },
                    act(if muted {
                        |_, cx| cx.emit(MuteRequested(false))
                    } else {
                        |_, cx| cx.emit(MuteRequested(true))
                    }),
                )
                .separator()
                .action(Icon::Close, "Close chat", act(|_, cx| cx.emit(ChatClosed)))
                .on_dismiss(act(|_, _| {}))
                .at(position),
        )
    }

    /// Asks for the open chat's profile, by its phone-number address when it
    /// has one: WhatsApp answers profile questions about a LID with less,
    /// and a LID shows no number.
    pub(super) fn request_profile(&mut self, cx: &mut Context<Self>) {
        let Some(chat) = self.log.read(cx).open_chat() else {
            return;
        };
        let request = ProfileRequested {
            chat: chat
                .addresses
                .iter()
                .find(|address| address.phone_number().is_some())
                .unwrap_or(&chat.id)
                .clone(),
            title: chat.title.clone(),
            verified: chat.verified,
        };
        cx.emit(request);
    }
}
