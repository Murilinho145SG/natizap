//! How attachments look inside a bubble.

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Div, FontWeight, ImageSource, ObjectFit, WeakEntity, div, img, px, rgb, rgba,
    svg,
};
use natizap_protocol::ContentKind;
use natizap_ui::Icon;
use natizap_ui::theme::{DEEP, MUTED, SCRIM, TEXT};

use crate::items::MediaView;
use crate::view::ConversationView;

/// The attachment of a bubble: an image for photos and stickers, a preview
/// with a play button for videos, and a row for documents. Videos play over
/// the window and documents open in the system's app for them; voice notes
/// get their own player (`voice_view`).
pub(crate) fn render_media(media: &MediaView, view: &WeakEntity<ConversationView>) -> AnyElement {
    match media.fetch.kind {
        ContentKind::Photo | ContentKind::Sticker => picture(media).into_any_element(),
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
            .cursor_pointer()
            .on_click(open(media, view))
            .into_any_element(),
        kind => div()
            .id("media")
            .min_w(px(220.0))
            .flex()
            .items_center()
            .gap_3()
            .p_2()
            .rounded(px(10.0))
            .bg(rgb(DEEP))
            .cursor_pointer()
            .on_click(open(media, view))
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
                    .child(div().font_weight(FontWeight::MEDIUM).child(kind.label()))
                    .children(media.detail.clone().map(|detail| {
                        div()
                            .truncate()
                            .text_size(px(12.5))
                            .text_color(rgb(MUTED))
                            .child(detail)
                    })),
            )
            .into_any_element(),
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
    div()
        .w(px(width))
        .h(px(height))
        .rounded(px(11.0))
        .overflow_hidden()
        .when(media.fetch.kind != ContentKind::Sticker, |frame| {
            frame.bg(rgb(DEEP))
        })
        .children(source.map(|source| {
            let fit = if media.fetch.kind == ContentKind::Sticker {
                ObjectFit::Contain
            } else {
                ObjectFit::Cover
            };
            img(source).size_full().object_fit(fit)
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
