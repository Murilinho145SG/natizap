//! Shared contacts, polls and a business's buttons inside a bubble.

use gpui::prelude::*;
use gpui::{
    AnyElement, ClipboardItem, Div, ElementId, FontWeight, SharedString, WeakEntity, div, px, rgb,
    rgba, svg,
};
use natizap_protocol::{ButtonAction, Buttons, Card, ChatId, Extra, Reply};
use natizap_ui::theme::Theme;
use natizap_ui::{Icon, avatar, separator};

use crate::poll::PollState;
use crate::poll_view::{Faces, poll_view};

use crate::view::{ChatRequested, ConversationView};

/// `extra` as the phone shows it above the text, or `None` for buttons,
/// which go under it ([`render_buttons`]). `id` must be unique among the
/// bubbles on screen.
pub(crate) fn render_extra(
    id: &str,
    extra: &Extra,
    poll: Option<&PollState>,
    faces: &Faces,
    view: &WeakEntity<ConversationView>,
    theme: Theme,
) -> Option<AnyElement> {
    Some(match extra {
        Extra::Cards(cards) => div()
            .min_w(px(240.0))
            .flex()
            .flex_col()
            .children(
                cards
                    .iter()
                    .enumerate()
                    .map(|(index, card)| contact(id, index, card, view, theme)),
            )
            .into_any_element(),
        Extra::Poll(question) => {
            poll_view(id, question, poll, faces, view, theme).into_any_element()
        }
        Extra::Buttons(_) => return None,
    })
}

/// A business's footer and buttons, under the text and time, one full
/// row each as the phone draws them. Links, calls, codes and quick
/// replies act when clicked, a quick reply answering `offered`, the
/// message they are under; lists only show.
pub(crate) fn render_buttons(
    id: &str,
    buttons: &Buttons,
    offered: &Reply,
    view: &WeakEntity<ConversationView>,
    theme: Theme,
) -> Div {
    let footer = buttons.footer.clone().map(|footer| {
        div()
            .pb_1()
            .text_size(px(12.5))
            .text_color(rgb(theme.muted))
            .child(footer)
    });
    let rows = buttons.buttons.iter().enumerate().map(|(index, button)| {
        let icon = match &button.action {
            ButtonAction::Url(_) => Some(Icon::Link),
            ButtonAction::Call(_) => Some(Icon::Call),
            ButtonAction::Copy(_) => Some(Icon::Copy),
            ButtonAction::List => Some(Icon::More),
            ButtonAction::Reply(_) => Some(Icon::Reply),
        };
        let action = button.action.clone();
        let acts = !matches!(action, ButtonAction::List);
        let (label, offered, view) = (button.label.clone(), offered.clone(), view.clone());
        div()
            .id((SharedString::from(format!("{id}-button")), index))
            .w_full()
            .py_2()
            .flex()
            .items_center()
            .justify_center()
            .gap_1p5()
            .border_t_1()
            .border_color(rgb(theme.line))
            .text_size(px(14.0))
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(theme.link))
            .children(icon.map(|icon| {
                svg()
                    .path(icon.path())
                    .size(px(15.0))
                    .text_color(rgb(theme.link))
            }))
            .child(button.label.clone())
            .when(acts, |row| {
                row.cursor_pointer()
                    .hover(|row| row.bg(rgba(theme.hover)))
                    .on_click(move |_, _, cx| match &action {
                        ButtonAction::Url(url) => {
                            tracing::debug!(url, "opening a button's link");
                            cx.open_url(url);
                        }
                        ButtonAction::Call(phone) => cx.open_url(&format!("tel:{phone}")),
                        ButtonAction::Copy(code) => {
                            cx.write_to_clipboard(ClipboardItem::new_string(code.clone()));
                        }
                        ButtonAction::Reply(answer) => {
                            let (label, answer, offered) =
                                (label.clone(), answer.clone(), offered.clone());
                            let _ = view.update(cx, |view, cx| {
                                view.answer(label, answer, offered, cx);
                            });
                        }
                        ButtonAction::List => {}
                    })
            })
    });
    div()
        .w_full()
        .min_w(px(240.0))
        .flex()
        .flex_col()
        .pt_1()
        .children(footer)
        .children(rows)
}

/// A shared contact: its name and number, and a way to message it.
fn contact(
    id: &str,
    index: usize,
    card: &Card,
    view: &WeakEntity<ConversationView>,
    theme: Theme,
) -> Div {
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
                                .text_color(rgb(theme.muted))
                                .child(number)
                        })),
                ),
        )
        .children(phone.map(|phone| {
            let view = view.clone();
            div().pt_1().child(separator(theme)).child(
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
                    .text_color(rgb(theme.link))
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
