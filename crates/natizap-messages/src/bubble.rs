//! How each row of a conversation looks.

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Div, FontStyle, FontWeight, HighlightStyle, Image, MouseButton, ObjectFit,
    SharedString, Stateful, StrikethroughStyle, StyledText, WeakEntity, div, img, px, relative,
    rgb, rgba, svg,
};
use natizap_protocol::{ContentKind, Mark};
use natizap_ui::theme::{
    ALERT, DEEP, LINE, MINT, MUTED, OUTGOING, PANEL, RAISED, SCRIM, TEXT, glass,
};
use natizap_ui::{Button, ButtonSize, ButtonVariant, Icon};
use std::sync::Arc;

use crate::items::{Body, Bubble, Item, MediaView, QuotePreview, Text};
use crate::media_view::render_media;
use crate::message::Delivery;
use crate::view::ConversationView;
use crate::voice::VoiceUi;
use crate::voice_view::render_voice;

/// Quick reactions offered on every bubble, as in WhatsApp.
pub(crate) const QUICK_REACTIONS: [&str; 6] = ["👍", "❤️", "😂", "😮", "😢", "🙏"];

pub(crate) fn render_item(
    item: &Item,
    view: &WeakEntity<ConversationView>,
    reacting: Option<&SharedString>,
    voice: &VoiceUi,
) -> AnyElement {
    match item {
        Item::Day(label) => div()
            .w_full()
            .flex()
            .justify_center()
            .pt_4()
            .pb_2()
            .child(
                div()
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .bg(rgb(PANEL))
                    .text_size(px(12.0))
                    .text_color(rgb(MUTED))
                    .child(label.clone()),
            )
            .into_any_element(),
        Item::Bubble(bubble) => render_bubble(
            bubble,
            view,
            reacting.is_some_and(|id| *id == bubble.id),
            voice,
        ),
    }
}

fn render_bubble(
    bubble: &Bubble,
    view: &WeakEntity<ConversationView>,
    reacting: bool,
    voice: &VoiceUi,
) -> AnyElement {
    let message = div()
        .max_w(relative(0.72))
        .flex()
        .flex_col()
        .gap_0p5()
        .when(bubble.from_me, Styled::items_end)
        .when(!bubble.from_me, Styled::items_start)
        .child(shape(bubble, view, voice).max_w_full())
        .when(!bubble.reactions.is_empty(), |message| {
            message.child(reaction_chips(bubble))
        });
    let actions = if reacting {
        quick_reactions(bubble, view).into_any_element()
    } else {
        hover_actions(bubble, view).into_any_element()
    };
    let menu = {
        let (view, id) = (view.clone(), bubble.id.clone());
        move |event: &gpui::MouseDownEvent, _: &mut gpui::Window, cx: &mut App| {
            let _ = view.update(cx, |view, cx| {
                view.open_message_menu(id.clone(), event.position, cx);
            });
        }
    };
    div()
        .id(bubble.id.clone())
        .group("bubble")
        .on_mouse_down(MouseButton::Right, menu)
        .w_full()
        .flex()
        .items_center()
        .gap_1()
        .pt(px(if bubble.starts_burst { 8.0 } else { 2.0 }))
        .when(bubble.from_me, Styled::justify_end)
        .map(|row| {
            // Actions sit on the open side of the bubble.
            if bubble.from_me {
                row.child(actions).child(message)
            } else {
                row.child(message).child(actions)
            }
        })
        .into_any_element()
}

/// The bubble itself: author, attachment, quote, content and time.
fn shape(bubble: &Bubble, view: &WeakEntity<ConversationView>, voice: &VoiceUi) -> Div {
    let meta = div()
        .flex()
        .items_center()
        .justify_end()
        .gap_1p5()
        .text_size(px(11.0))
        .text_color(rgb(MUTED))
        .when(
            matches!(bubble.body, Body::Content { edited: true, .. }),
            |meta| meta.child("edited"),
        )
        .child(bubble.time.clone())
        .when(bubble.from_me, |meta| {
            meta.child(delivery_mark(bubble.delivery))
        });
    // A voice note carries no text, so its time and ticks sit inside the
    // player instead of on a line of their own, as on the phone.
    let voice_note = bubble.media.as_ref().filter(|media| {
        matches!(
            media.fetch.kind,
            ContentKind::VoiceMessage | ContentKind::Audio
        )
    });
    if let Some(media) = voice_note {
        return padded(bubble).child(render_voice(media, voice, meta, view));
    }
    let picture = bubble
        .media
        .as_ref()
        .filter(|media| matches!(media.fetch.kind, ContentKind::Photo | ContentKind::Video));
    if let Some(media) = picture {
        return picture_bubble(bubble, media, meta, view);
    }

    let body = match &bubble.body {
        Body::Deleted => div()
            .italic()
            .text_color(rgb(MUTED))
            .child("This message was deleted"),
        Body::Content { media, text, .. } => div()
            .flex()
            .flex_col()
            .gap_0p5()
            .children(media.map(|media| {
                div()
                    .text_size(px(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(MUTED))
                    .child(media)
            }))
            .children(text.as_ref().map(styled_text)),
    };

    padded(bubble)
        .children(bubble.media.as_ref().map(|media| render_media(media, view)))
        .children(bubble.quote.as_ref().map(quoted))
        // Time sits beside short text and drops below text that fills the
        // bubble, as in WhatsApp.
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_end()
                .gap_x_2()
                // Without a max width the text keeps its unwrapped width and
                // overflows the bubble.
                .child(body.min_w_0().max_w_full())
                .child(meta.ml_auto().pb(px(1.0))),
        )
}

/// A photo or video filling its bubble but for a thin rim, as on the
/// phone: the time sits on the picture, or beside the caption if any.
fn picture_bubble(
    bubble: &Bubble,
    media: &MediaView,
    meta: Div,
    view: &WeakEntity<ConversationView>,
) -> Div {
    let caption = match &bubble.body {
        Body::Content {
            text: Some(text), ..
        } => Some(text),
        _ => None,
    };
    let picture = div().relative().child(render_media(media, view));
    let (picture, caption) = match caption {
        Some(text) => (
            picture,
            Some(
                div()
                    .px(px(7.0))
                    .pb(px(3.0))
                    .flex()
                    .flex_wrap()
                    .items_end()
                    .gap_x_2()
                    .child(styled_text(text).into_any_element())
                    .child(meta.ml_auto().pb(px(1.0))),
            ),
        ),
        None => (
            picture.child(
                div()
                    .absolute()
                    .bottom(px(6.0))
                    .right(px(6.0))
                    .px_1p5()
                    .rounded_full()
                    .bg(rgba(SCRIM))
                    .child(meta.text_color(rgb(TEXT))),
            ),
            None,
        ),
    };
    frame(bubble)
        .p(px(3.0))
        .gap_1()
        .children(author(bubble).map(|name| name.px(px(7.0)).pt(px(2.0))))
        .children(bubble.quote.as_ref().map(quoted))
        .child(picture)
        .children(caption)
}

/// [`frame`] with the room text needs around it.
fn padded(bubble: &Bubble) -> Div {
    frame(bubble)
        .gap_1()
        .px(px(10.0))
        .py(px(6.0))
        .children(author(bubble))
}

/// The bubble's background and corners.
fn frame(bubble: &Bubble) -> Div {
    div()
        .flex()
        .flex_col()
        .rounded(px(14.0))
        .bg(rgb(if bubble.from_me { OUTGOING } else { RAISED }))
        .map(glass)
        .text_size(px(14.5))
        .line_height(relative(1.4))
        .map(|shape| match (bubble.starts_burst, bubble.from_me) {
            // The corner nearest the sender stays sharp on a burst's first
            // bubble, pointing at who said it.
            (true, true) => shape.rounded_tr(px(4.0)),
            (true, false) => shape.rounded_tl(px(4.0)),
            _ => shape,
        })
}

/// Who wrote a group message.
fn author(bubble: &Bubble) -> Option<Div> {
    bubble.author.clone().map(|(name, color)| {
        div()
            .text_size(px(13.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(color))
            .child(name)
    })
}

/// Reply and react buttons, shown while the pointer is over the bubble.
fn hover_actions(bubble: &Bubble, view: &WeakEntity<ConversationView>) -> Div {
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
fn quick_reactions(bubble: &Bubble, view: &WeakEntity<ConversationView>) -> Stateful<Div> {
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
}

fn reaction_chips(bubble: &Bubble) -> Div {
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

/// A quoted message: who wrote it over one line of what they said, marked
/// by a bar in their color.
fn quoted(quote: &QuotePreview) -> Div {
    quote_block(
        quote.author.clone(),
        quote.color,
        quote.text.clone(),
        quote.kind,
        quote.thumbnail.clone(),
    )
}

/// A quoted message: a bar in the author's color, the author, an icon for
/// what it carries and a line of it, and a photo or video's preview on the
/// right, as in WhatsApp.
pub(crate) fn quote_block(
    author: SharedString,
    color: u32,
    text: SharedString,
    kind: ContentKind,
    thumbnail: Option<Arc<Image>>,
) -> Div {
    let icon = match kind {
        ContentKind::Photo | ContentKind::Sticker => Some(Icon::Camera),
        ContentKind::Video => Some(Icon::Video),
        ContentKind::VoiceMessage => Some(Icon::Mic),
        ContentKind::Audio => Some(Icon::Waveform),
        ContentKind::Document => Some(Icon::File),
        ContentKind::Text | ContentKind::Location | ContentKind::Contact | ContentKind::Poll => {
            None
        }
    };
    div()
        .flex()
        .rounded(px(8.0))
        .overflow_hidden()
        .bg(rgb(DEEP))
        .child(div().w(px(3.0)).flex_none().bg(rgb(color)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .px_2()
                .py_1()
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(color))
                        .child(author),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_size(px(13.0))
                        .text_color(rgb(MUTED))
                        .children(icon.map(|icon| {
                            svg()
                                .path(icon.path())
                                .size(px(14.0))
                                .flex_none()
                                .text_color(rgb(MUTED))
                        }))
                        .child(div().min_w_0().truncate().child(text)),
                ),
        )
        .children(thumbnail.map(|thumbnail| {
            img(thumbnail)
                .w(px(48.0))
                .flex_none()
                .self_stretch()
                .rounded_r(px(8.0))
                .object_fit(ObjectFit::Cover)
        }))
}

/// Where a sent message stands, as WhatsApp shows it: a clock while
/// sending, one check when sent, two when delivered, tinted once read.
fn delivery_mark(delivery: Delivery) -> AnyElement {
    let (icon, color) = match delivery {
        Delivery::Failed => {
            return div()
                .flex()
                .items_center()
                .gap_1()
                .text_color(rgb(ALERT))
                .child(
                    svg()
                        .path(Icon::Alert.path())
                        .size(px(13.0))
                        .text_color(rgb(ALERT)),
                )
                .child("Not sent")
                .into_any_element();
        }
        Delivery::Sending => (Icon::Clock, MUTED),
        Delivery::Sent => (Icon::Check, MUTED),
        Delivery::Delivered => (Icon::Checks, MUTED),
        Delivery::Read => (Icon::Checks, MINT),
    };
    svg()
        .path(icon.path())
        .size(px(15.0))
        .text_color(rgb(color))
        .into_any_element()
}

fn styled_text(text: &Text) -> StyledText {
    let highlights = text.marks.iter().map(|(range, mark)| {
        let style = match mark {
            Mark::Bold => HighlightStyle {
                font_weight: Some(FontWeight::BOLD),
                ..Default::default()
            },
            Mark::Italic => HighlightStyle {
                font_style: Some(FontStyle::Italic),
                ..Default::default()
            },
            Mark::Strikethrough => HighlightStyle {
                strikethrough: Some(StrikethroughStyle {
                    thickness: px(1.0),
                    color: None,
                }),
                ..Default::default()
            },
            Mark::Code => HighlightStyle {
                background_color: Some(rgb(DEEP).into()),
                ..Default::default()
            },
            Mark::Quote => HighlightStyle {
                color: Some(rgb(MUTED).into()),
                ..Default::default()
            },
        };
        (range.clone(), style)
    });
    StyledText::new(text.text.clone()).with_highlights(highlights)
}
