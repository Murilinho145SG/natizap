//! The bar over a conversation: who it is with.

use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{Context, Div, FontWeight, Stateful, div, px, rgb};
use natizap_ui::theme::{LINE, MUTED, PANEL};
use natizap_ui::{avatar, verified_seal};

use super::{ConversationView, ProfileRequested};
use crate::log::OpenChat;

/// The chat's picture, name and, for a person, their number. Clicking it
/// asks for the chat's profile.
pub(super) fn header(
    chat: &OpenChat,
    picture: Option<PathBuf>,
    cx: &mut Context<ConversationView>,
) -> Stateful<Div> {
    let (verified, title) = (chat.verified, chat.title.clone());
    // The phone-number address, when the chat has one: WhatsApp answers
    // profile questions about a LID with less, and a LID shows no number.
    let address = chat
        .addresses
        .iter()
        .find(|address| address.phone_number().is_some())
        .unwrap_or(&chat.id)
        .clone();
    let subtitle = if chat.id.is_group() {
        Some("Group".to_owned())
    } else {
        chat.id
            .phone_number()
            .filter(|phone| *phone != chat.title.as_ref())
    };
    div()
        .id("chat-header")
        .cursor_pointer()
        .on_click(cx.listener(move |_, _, _, cx| {
            cx.emit(ProfileRequested {
                chat: address.clone(),
                title: title.clone(),
                verified,
            });
        }))
        .h(px(60.0))
        .flex_none()
        .flex()
        .items_center()
        .gap_3()
        .px_4()
        .bg(rgb(PANEL))
        .border_b_1()
        .border_color(rgb(LINE))
        .child(avatar(chat.id.as_str(), &chat.title, picture, px(38.0)))
        .child(
            div()
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
                        .when(chat.verified, |title| title.child(verified_seal(16.0))),
                )
                .children(subtitle.map(|subtitle| {
                    div()
                        .text_size(px(12.5))
                        .text_color(rgb(MUTED))
                        .child(subtitle)
                })),
        )
}
