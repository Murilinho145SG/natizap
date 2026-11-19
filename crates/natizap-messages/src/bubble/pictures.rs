//! Bubbles that are mostly a picture: photos and videos filling their
//! bubble, and stickers with no bubble at all.

use gpui::prelude::*;
use gpui::{Div, WeakEntity, div, px, rgb, rgba};
use natizap_ui::theme::Theme;

use super::{author, forwarded, frame, jump_to_quoted};
use crate::items::{Body, Bubble, MediaView};
use crate::media_view::render_media;
use crate::rich_text::{TextFocus, rich_text, selectable};
use crate::view::ConversationView;

/// A sticker, as the phone shows it: no bubble, just the sticker, with
/// its author's name above it in groups and its time on a dark pill over
/// its corner.
pub(super) fn sticker_bubble(
    bubble: &Bubble,
    media: &MediaView,
    meta: Div,
    view: &WeakEntity<ConversationView>,
    downloading: Option<f32>,
    theme: Theme,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .when(bubble.from_me, Styled::items_end)
        .children(author(bubble, view, theme).map(Styled::px_1))
        .children(forwarded(bubble, theme).map(Styled::px_1))
        .children(
            bubble
                .quote
                .as_ref()
                .map(|quote| jump_to_quoted(quote, view, theme)),
        )
        .child(
            div()
                .relative()
                .child(render_media(media, view, downloading, theme))
                .child(
                    div()
                        .absolute()
                        .bottom(px(4.0))
                        .right(px(4.0))
                        .px_1p5()
                        .rounded_full()
                        .bg(rgba(theme.scrim))
                        .child(meta.text_color(rgb(theme.text))),
                ),
        )
}

/// The bubble's padding around a picture.
const PICTURE_PADDING: f32 = 3.0;

/// A photo or video filling its bubble but for a thin rim, as on the
/// phone: the time sits on the picture, or beside the caption if any.
pub(super) fn picture_bubble(
    bubble: &Bubble,
    media: &MediaView,
    meta: Div,
    view: &WeakEntity<ConversationView>,
    focus: &TextFocus,
    downloading: Option<f32>,
    theme: Theme,
) -> Div {
    let caption = match &bubble.body {
        Body::Content {
            text: Some(text), ..
        } => Some(text),
        _ => None,
    };
    let picture = div()
        .relative()
        .child(render_media(media, view, downloading, theme));
    let (picture, caption) = match caption {
        Some(text) => {
            let (shown, hits) = rich_text(&bubble.id, text, view, focus, theme);
            // As wide as the bubble, which is as wide as the picture, so
            // long captions wrap under it instead of spilling past it.
            let caption = div()
                .w_full()
                .px(px(7.0))
                .pb(px(3.0))
                .flex()
                .flex_wrap()
                .items_end()
                .gap_x_2()
                // Text measures itself at its longest line; held to the
                // caption's width, it wraps there instead of running past
                // the bubble, and a short caption still leaves the time room
                // beside it.
                .child(div().max_w_full().min_w_0().child(shown))
                .child(meta.ml_auto().pb(px(1.0)));
            (picture, Some(selectable(caption, hits, view)))
        }
        None => (
            picture.child(
                div()
                    .absolute()
                    .bottom(px(6.0))
                    .right(px(6.0))
                    .px_1p5()
                    .rounded_full()
                    .bg(rgba(theme.scrim))
                    .child(meta.text_color(rgb(theme.text))),
            ),
            None,
        ),
    };
    // A width of its own, the picture's and its padding, narrowing with
    // the window: a caption measured against anything wider wraps into
    // more lines than were laid out and spills out of the bubble.
    frame(bubble, theme)
        .w(px(media.size.0 + 2.0 * PICTURE_PADDING))
        .max_w_full()
        .p(px(PICTURE_PADDING))
        .gap_1()
        .children(author(bubble, view, theme).map(|name| name.px(px(7.0)).pt(px(2.0))))
        .children(forwarded(bubble, theme).map(|line| line.px(px(7.0))))
        .children(
            bubble
                .quote
                .as_ref()
                .map(|quote| jump_to_quoted(quote, view, theme)),
        )
        .child(picture)
        .children(caption)
}
