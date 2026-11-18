//! The favorite stickers to pick one to send, in a grid that floats over
//! the conversation from the composer.

use gpui::prelude::*;
use gpui::{
    Context, Div, Entity, EventEmitter, FontWeight, ObjectFit, SharedString, Stateful, Window, div,
    img, px, rgb, svg,
};
use natizap_ui::theme::{DEEP, MUTED, PANEL, RAISED, TEXT, glass};
use natizap_ui::{Icon, Tooltip};

use crate::shelf::{Favorite, StickerShelf};

/// Stickers in a row of the grid.
const COLUMNS: usize = 5;
/// Side of a sticker in the grid.
const CELL: f32 = 68.0;
/// Space between stickers.
const GAP: f32 = 6.0;

/// The user picked a favorite sticker to send.
#[derive(Debug, Clone)]
pub struct StickerPicked(pub Favorite);

/// The grid of favorite stickers. Emits [`StickerPicked`].
#[derive(Debug)]
pub struct StickerPicker {
    shelf: Entity<StickerShelf>,
}

impl EventEmitter<StickerPicked> for StickerPicker {}

impl StickerPicker {
    /// Shows `shelf`'s favorites, following their changes.
    pub fn new(shelf: Entity<StickerShelf>, cx: &mut Context<Self>) -> Self {
        cx.observe(&shelf, |_, _, cx| cx.notify()).detach();
        Self { shelf }
    }
}

impl Render for StickerPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let favorites: Vec<Favorite> = self.shelf.read(cx).favorites().cloned().collect();
        #[expect(clippy::cast_precision_loss, reason = "a handful of columns")]
        let width = COLUMNS as f32 * CELL + (COLUMNS - 1) as f32 * GAP + 24.0;
        div()
            .id("sticker-picker")
            .w(px(width))
            .max_h(px(360.0))
            .flex()
            .flex_col()
            .rounded_xl()
            .bg(rgb(PANEL))
            .map(glass)
            .child(
                div()
                    .px_3()
                    .pt_3()
                    .pb_2()
                    .text_size(px(13.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(MUTED))
                    .child("Favorite stickers"),
            )
            .child(if favorites.is_empty() {
                div()
                    .px_3()
                    .pb_4()
                    .text_size(px(13.0))
                    .text_color(rgb(MUTED))
                    .child(
                        "No favorites yet. Click a sticker in a chat and add it \
                         to your favorites.",
                    )
                    .into_any_element()
            } else {
                div()
                    .id("sticker-grid")
                    .px_3()
                    .pb_3()
                    .flex()
                    .flex_wrap()
                    .gap(px(GAP))
                    .overflow_y_scroll()
                    .children(
                        favorites
                            .into_iter()
                            .enumerate()
                            .map(|(index, favorite)| cell(index, favorite, cx)),
                    )
                    .into_any_element()
            })
    }
}

/// A sticker to pick: the sticker itself, lit on hover.
fn cell(index: usize, favorite: Favorite, cx: &mut Context<StickerPicker>) -> Stateful<Div> {
    let shown = favorite.file.clone().map(|file| {
        img(file)
            .id(SharedString::from(format!(
                "favorite-{}",
                favorite.sticker.hash
            )))
            .size_full()
            .object_fit(ObjectFit::Contain)
            .into_any_element()
    });
    let sendable = favorite.sendable();
    div()
        .id(("favorite-sticker", index))
        .size(px(CELL))
        .flex()
        .items_center()
        .justify_center()
        .p_1()
        .rounded_lg()
        .child(shown.unwrap_or_else(|| {
            // Still downloading, a Lottie sticker natizap cannot draw, or
            // one only the phone still has.
            svg()
                .path(Icon::Sticker.path())
                .size(px(24.0))
                .text_color(rgb(MUTED))
                .into_any_element()
        }))
        .when(sendable, |cell| {
            cell.cursor_pointer()
                .hover(|cell| cell.bg(rgb(RAISED)))
                .active(|cell| cell.bg(rgb(DEEP)))
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(StickerPicked(favorite.clone()));
                }))
        })
        .when(!sendable, |cell| {
            cell.opacity(0.4).tooltip(Tooltip::text(
                "Only your phone still has this one. Add it again from a chat to use it here.",
            ))
        })
        .text_color(rgb(TEXT))
}
