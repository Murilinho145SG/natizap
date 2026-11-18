//! The strip of small pictures under the carousel's stage: every photo and
//! video of the chat, oldest on the left, what shows lit and kept in view.

use gpui::prelude::*;
use gpui::{Context, Div, ObjectFit, SharedString, div, img, px, rgb, svg};
use natizap_protocol::ContentKind;
use natizap_ui::Icon;
use natizap_ui::theme::{MUTED, RAISED, TEXT};

use crate::carousel::Carousel;
use crate::view::ConversationView;

/// Height the strip takes under the stage.
pub(super) const HEIGHT: f32 = 76.0;
/// Side of one small picture.
const TILE: f32 = 52.0;

/// Scrolls the strip so what shows is in view.
pub(super) fn reveal(carousel: &Carousel) {
    if let Some(at) = carousel.at() {
        // The strip runs oldest first; the shelf, newest first.
        carousel
            .strip
            .scroll_to_item(carousel.shelf().len().saturating_sub(at + 1));
    }
}

/// The strip, empty until the chat's photos and videos are read.
pub(super) fn strip(carousel: &Carousel, cx: &mut Context<ConversationView>) -> Div {
    let shelf = carousel.shelf();
    let tiles = shelf.iter().enumerate().rev().map(|(index, item)| {
        let current = carousel.at() == Some(index);
        let video = item.media.fetch.kind == ContentKind::Video;
        let picture = item.tile_picture(carousel.made.get(&index));
        let shown = picture.is_some();
        div()
            .id(SharedString::from(format!(
                "strip-{}",
                item.media.fetch.message_id
            )))
            .relative()
            .flex_none()
            .size(px(TILE))
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .rounded_md()
            .bg(rgb(RAISED))
            .border_2()
            .border_color(if current {
                rgb(TEXT)
            } else {
                gpui::transparent_black().into()
            })
            .opacity(if current { 1.0 } else { 0.55 })
            .cursor_pointer()
            .hover(|tile| tile.opacity(1.0))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.jump_carousel(index, cx);
            }))
            .child(match picture {
                Some(picture) => img(picture)
                    .size(px(TILE))
                    .flex_none()
                    .object_fit(ObjectFit::Cover)
                    .into_any_element(),
                None => svg()
                    .path(if video { Icon::Video } else { Icon::Camera }.path())
                    .size(px(18.0))
                    .text_color(rgb(MUTED))
                    .into_any_element(),
            })
            .when(video && shown, |tile| {
                tile.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            svg()
                                .path(Icon::Play.path())
                                .size(px(16.0))
                                .text_color(rgb(TEXT)),
                        ),
                )
            })
    });
    div()
        .flex_none()
        .h(px(HEIGHT))
        .flex()
        .justify_center()
        .child(
            div()
                .id("carousel-strip")
                .max_w_full()
                .h_full()
                .px_4()
                .flex()
                .items_center()
                .gap_1p5()
                .overflow_x_scroll()
                .track_scroll(&carousel.strip)
                .children(tiles),
        )
}
