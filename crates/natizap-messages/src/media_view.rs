//! How attachments look inside a bubble.

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Div, FontWeight, ImageSource, ObjectFit, Stateful, WeakEntity, div, img, px,
    rgb, rgba, svg,
};
use natizap_protocol::ContentKind;
use natizap_ui::Icon;
use natizap_ui::theme::{DEEP, MUTED, SCRIM, TEXT};

use crate::items::MediaView;
use crate::view::ConversationView;

/// Width of a document's card.
const DOCUMENT_WIDTH: f32 = 300.0;
/// Height of the first page shown on a document's card.
const DOCUMENT_PREVIEW: f32 = 110.0;

/// The attachment of a bubble: an image for photos and stickers, a preview
/// with a play button for videos, and a row for documents. Videos play over
/// the window and documents open in the system's app for them; voice notes
/// get their own player (`voice_view`).
pub(crate) fn render_media(media: &MediaView, view: &WeakEntity<ConversationView>) -> AnyElement {
    match media.fetch.kind {
        ContentKind::Photo => picture(media)
            .id("media")
            .map(pressable)
            .on_click({
                let (media, view) = (media.clone(), view.clone());
                move |_, _, cx| {
                    let _ = view.update(cx, |view, cx| view.view_picture(&media, cx));
                }
            })
            .into_any_element(),
        ContentKind::Sticker => picture(media).into_any_element(),
        ContentKind::Video => picture(media)
            .relative()
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(round_icon(Icon::Play, 48.0)),
            )
            .children(media.detail.clone().map(|duration| {
                div()
                    .absolute()
                    .bottom_1()
                    .left_2()
                    .text_size(px(12.0))
                    .text_color(rgb(TEXT))
                    .child(duration)
            }))
            .id("media")
            .map(pressable)
            .on_click(open(media, view))
            .into_any_element(),
        kind => {
            let row = div()
                .flex()
                .items_center()
                .gap_3()
                .p_2()
                .child(round_icon(
                    if kind == ContentKind::Document {
                        Icon::File
                    } else {
                        Icon::Play
                    },
                    36.0,
                ))
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .child(media.name.clone().unwrap_or_else(|| kind.label().into())),
                        )
                        .children(media.detail.clone().map(|detail| {
                            div()
                                .truncate()
                                .text_size(px(12.5))
                                .text_color(rgb(MUTED))
                                .child(detail)
                        })),
                );
            div()
                .id("media")
                .w(px(DOCUMENT_WIDTH))
                .max_w_full()
                .flex()
                .flex_col()
                .rounded(px(10.0))
                .overflow_hidden()
                .bg(rgb(DEEP))
                .map(pressable)
                .on_click(open(media, view))
                // A document's first page, as its sender's phone drew it.
                .children(
                    media
                        .preview
                        .clone()
                        .filter(|_| kind == ContentKind::Document)
                        .map(|preview| {
                            img(preview)
                                .w_full()
                                .h(px(DOCUMENT_PREVIEW))
                                .rounded_t(px(10.0))
                                .object_fit(ObjectFit::Cover)
                        }),
                )
                .child(row)
                .into_any_element()
        }
    }
}

/// The downloaded image, else a video's still, else the inline preview,
/// else an empty frame. A video's file is not an image, so it never shows.
fn picture(media: &MediaView) -> Div {
    let (width, height) = media.size;
    let is_image = matches!(media.fetch.kind, ContentKind::Photo | ContentKind::Sticker);
    let source: Option<ImageSource> = match (&media.file, &media.preview) {
        (Some(file), _) if is_image && file.exists() => Some(file.clone().into()),
        _ if media.poster.is_some() => media.poster.clone().map(Into::into),
        (_, Some(preview)) => Some(preview.clone().into()),
        _ => None,
    };
    // Its own size when the chat has room, narrower in a narrow window,
    // keeping its shape either way.
    div()
        .w(px(width))
        .max_w_full()
        .aspect_ratio(width / height)
        .rounded(px(11.0))
        .overflow_hidden()
        .when(media.fetch.kind != ContentKind::Sticker, |frame| {
            frame.bg(rgb(DEEP))
        })
        // Nothing to show yet, such as a photo still downloading that came
        // without a preview: what it is, rather than an empty frame. Videos
        // show their play button there already.
        .when(
            source.is_none() && media.fetch.kind == ContentKind::Photo,
            |frame| {
                frame.flex().items_center().justify_center().child(
                    svg()
                        .path(Icon::Camera.path())
                        .size(px(32.0))
                        .text_color(rgb(MUTED)),
                )
            },
        )
        .children(source.map(|source| {
            let fit = if media.fetch.kind == ContentKind::Sticker {
                ObjectFit::Contain
            } else {
                ObjectFit::Cover
            };
            // Rounded on the image itself: the frame's overflow clips to
            // its rectangle, not its rounded corners.
            // The frame's shape, given outright: left to itself, an image
            // sized in relative units takes the picture's own shape.
            img(source)
                .size_full()
                .aspect_ratio(width / height)
                .rounded(px(11.0))
                .object_fit(fit)
        }))
}

fn round_icon(icon: Icon, size: f32) -> Div {
    div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(rgba(SCRIM))
        .child(
            svg()
                .path(icon.path())
                .size(px(size * 0.45))
                .text_color(rgb(TEXT)),
        )
}

fn open(
    media: &MediaView,
    view: &WeakEntity<ConversationView>,
) -> impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut App) + 'static {
    let (media, view) = (media.clone(), view.clone());
    move |_, _, cx| {
        // The view is gone only while the window closes.
        let _ = view.update(cx, |view, cx| view.open_media(&media, cx));
    }
}

/// `media` showing it can be clicked: the hand, lit on hover, dimmed while
/// pressed.
fn pressable(media: Stateful<Div>) -> Stateful<Div> {
    media
        .cursor_pointer()
        .hover(|media| media.opacity(0.88))
        .active(|media| media.opacity(0.75))
}
