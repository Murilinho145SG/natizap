//! Shared contacts and polls inside a bubble.

use gpui::prelude::*;
use gpui::{AnyElement, Div, ElementId, FontWeight, WeakEntity, div, px, rgb};
use natizap_protocol::{Card, ChatId, Extra, Poll};
use natizap_ui::theme::{LINE, LINK, MUTED, TEXT};
use natizap_ui::{avatar, separator};

use crate::view::{ChatRequested, ConversationView};

/// `extra` as the phone shows it. `id` must be unique among the bubbles on
/// screen.
pub(crate) fn render_extra(
    id: &str,
    extra: &Extra,
    view: &WeakEntity<ConversationView>,
) -> AnyElement {
    match extra {
        Extra::Cards(cards) => div()
            .min_w(px(240.0))
            .flex()
            .flex_col()
            .children(
                cards
                    .iter()
                    .enumerate()
                    .map(|(index, card)| contact(id, index, card, view)),
            )
            .into_any_element(),
        Extra::Poll(poll) => poll_view(poll).into_any_element(),
    }
}

/// A shared contact: its name and number, and a way to message it.
fn contact(id: &str, index: usize, card: &Card, view: &WeakEntity<ConversationView>) -> Div {
    let phone = card
        .phone
        .as_ref()
        .map(|digits| ChatId::new(&format!("{digits}@s.whatsapp.net")));
    let number = phone.as_ref().and_then(ChatId::phone_number);
    let seed = phone.as_ref().map_or(card.name.as_str(), ChatId::as_str);
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .py_1()
                .child(avatar(seed, &card.name, None, px(40.0)))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .truncate()
                                .child(card.name.clone()),
                        )
                        .children(number.map(|number| {
                            div()
                                .text_size(px(12.5))
                                .text_color(rgb(MUTED))
                                .child(number)
                        })),
                ),
        )
        .children(phone.map(|phone| {
            let view = view.clone();
            div().pt_1().child(separator()).child(
                div()
                    .id(ElementId::NamedInteger(
                        format!("{id}-card").into(),
                        index as u64,
                    ))
                    .py_2()
                    .flex()
                    .justify_center()
                    .text_size(px(14.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(LINK))
                    .cursor_pointer()
                    .hover(Styled::underline)
                    .on_click(move |_, _, cx| {
                        let phone = phone.clone();
                        let _ = view.update(cx, |_, cx| cx.emit(ChatRequested { phone }));
                    })
                    .child("Message"),
            )
        }))
}

/// A poll's question and choices. Votes are not shown yet: reading them
/// means decrypting each vote with the poll's secret.
fn poll_view(poll: &Poll) -> Div {
    let hint = if poll.multiple {
        "Select one or more"
    } else {
        "Select one"
    };
    div()
        .min_w(px(240.0))
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(poll.question.clone()),
        )
        .child(div().text_size(px(12.5)).text_color(rgb(MUTED)).child(hint))
        .children(poll.options.iter().map(|option| {
            div()
                .flex()
                .items_center()
                .gap_2p5()
                .py_1()
                .child(
                    div()
                        .size(px(18.0))
                        .flex_none()
                        .border_2()
                        .border_color(rgb(LINE))
                        .when(poll.multiple, |mark| mark.rounded(px(4.0)))
                        .when(!poll.multiple, Styled::rounded_full),
                )
                .child(div().text_color(rgb(TEXT)).child(option.clone()))
        }))
}
