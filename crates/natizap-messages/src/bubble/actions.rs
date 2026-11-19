//! What shows beside and under a bubble: the reply and react buttons
//! under the pointer, the quick reactions, and the reactions it got.

use gpui::prelude::*;
use gpui::{App, Div, Stateful, WeakEntity, div, px, rgb};
use natizap_ui::theme::{LINE, MUTED, PANEL, RAISED, glass};
use natizap_ui::{Button, ButtonSize, ButtonVariant, Icon};

use super::QUICK_REACTIONS;
use crate::items::Bubble;
use crate::view::ConversationView;

/// Reply and react buttons, shown while the pointer is over the bubble.
pub(super) fn hover_actions(bubble: &Bubble, view: &WeakEntity<ConversationView>) -> Div {
    div()
        .flex()
        .invisible()
        .group_hover("bubble", Styled::visible)
        .child(hover_button("react", Icon::Emoji, "React").on_click({
            let (view, id) = (view.clone(), bubble.id.clone());
            move |_, _, cx| {
                let _ = view.update(cx, |view, cx| view.toggle_reactions(&id, cx));
            }
        }))
        .child(hover_button("reply", Icon::Reply, "Reply").on_click({
            let (view, bubble) = (view.clone(), bubble.clone());
            move |_, _, cx| reply_to(&view, &bubble, cx)
        }))
}

fn hover_button(id: &'static str, icon: Icon, name: &'static str) -> Button {
    Button::new(id)
        .icon(icon)
        .variant(ButtonVariant::Ghost)
        .size(ButtonSize::Small)
        .round()
        .tooltip(name)
}

/// The quick reaction strip; this account's current reaction is
/// highlighted and picking it again takes it back.
pub(super) fn quick_reactions(
    bubble: &Bubble,
    view: &WeakEntity<ConversationView>,
) -> Stateful<Div> {
    div()
        .id("quick-reactions")
        .on_mouse_down_out({
            let view = view.clone();
            move |_, _, cx| {
                let _ = view.update(cx, ConversationView::close_popups);
            }
        })
        .flex()
        .gap_0p5()
        .p_0p5()
        .rounded_full()
        .bg(rgb(PANEL))
        .map(glass)
        .children(QUICK_REACTIONS.iter().map(|&emoji| {
            let mine = bubble.my_reaction.as_deref() == Some(emoji);
            div()
                .id(emoji)
                .size(px(32.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .when(mine, |button| button.bg(rgb(RAISED)))
                .hover(|style| style.bg(rgb(RAISED)))
                .on_click({
                    let (view, bubble) = (view.clone(), bubble.clone());
                    move |_, _, cx| {
                        let _ = view.update(cx, |view, cx| view.react(&bubble, emoji, cx));
                    }
                })
                .text_size(px(20.0))
                .child(emoji)
        }))
        // Any other emoji, from the full picker, as on the phone.
        .child(
            Button::new("more-reactions")
                .icon(Icon::Plus)
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Small)
                .round()
                .tooltip("More reactions")
                .on_click({
                    let (view, id) = (view.clone(), bubble.id.clone());
                    move |event, _, cx| {
                        let position = event.position();
                        let _ = view.update(cx, |view, cx| {
                            view.open_reaction_picker(id.clone(), position, cx);
                        });
                    }
                }),
        )
}

pub(super) fn reaction_chips(bubble: &Bubble) -> Div {
    div()
        .flex()
        .gap_1()
        .children(bubble.reactions.iter().map(|chip| {
            div()
                .flex()
                .items_center()
                .gap_1()
                .px_1p5()
                .rounded_full()
                .bg(rgb(PANEL))
                .border_1()
                .border_color(rgb(LINE))
                .text_size(px(13.0))
                .child(chip.emoji.clone())
                .when(chip.count > 1, |chip_view| {
                    chip_view.child(div().text_color(rgb(MUTED)).child(chip.count.to_string()))
                })
        }))
}

fn reply_to(view: &WeakEntity<ConversationView>, bubble: &Bubble, cx: &mut App) {
    // The view is gone only while the window closes.
    let _ = view.update(cx, |view, cx| view.reply_to(bubble, cx));
}
