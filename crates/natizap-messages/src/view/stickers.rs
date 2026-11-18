//! Stickers in the conversation: the picker the app hands it, floating over
//! the composer, and keeping the stickers people send among favorites.

use std::collections::HashSet;
use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{AnyView, Context, WeakEntity, deferred, div, relative};
use natizap_protocol::{ContentKind, Sticker};
use natizap_ui::motion::rise_in;
use natizap_ui::{Icon, Menu};

use super::ConversationView;
use crate::items::{Bubble, MediaView};

/// The sticker picker the app handed over, and whether it is open.
#[derive(Debug)]
pub(super) struct StickerPicking {
    picker: AnyView,
    open: bool,
}

/// The user asked to keep a sticker among favorites, or take it out.
#[derive(Debug, Clone)]
pub struct StickerFavoriteToggled {
    /// The sticker.
    pub sticker: Sticker,
    /// Where its file already is on disk, when it is.
    pub file: Option<PathBuf>,
    /// Whether it is to be a favorite.
    pub favorite: bool,
}

impl ConversationView {
    /// The favorite stickers to pick from, shown from the composer's
    /// sticker button.
    pub fn set_sticker_picker(&mut self, picker: AnyView) {
        self.sticker_picker = Some(StickerPicking {
            picker,
            open: false,
        });
    }

    /// Closes the sticker picker, as after a sticker was picked.
    pub fn close_sticker_picker(&mut self, cx: &mut Context<Self>) {
        if self.picking_stickers() {
            self.close_stickers();
            cx.notify();
        }
    }

    /// Whether the sticker picker is open.
    pub(super) fn picking_stickers(&self) -> bool {
        self.sticker_picker
            .as_ref()
            .is_some_and(|picking| picking.open)
    }

    pub(super) fn close_stickers(&mut self) {
        if let Some(picking) = &mut self.sticker_picker {
            picking.open = false;
        }
    }

    /// The hashes of the favorite stickers, to offer adding or removing.
    pub fn set_favorite_stickers(&mut self, hashes: HashSet<String>, cx: &mut Context<Self>) {
        self.favorite_stickers = hashes;
        cx.notify();
    }

    pub(super) fn toggle_sticker_picker(&mut self, cx: &mut Context<Self>) {
        if let Some(picking) = &mut self.sticker_picker {
            picking.open = !picking.open;
        }
        self.picking_emoji = false;
        cx.notify();
    }

    /// The sticker picker, floating above the composer while open.
    pub(super) fn sticker_popover(&self) -> Option<impl IntoElement + use<>> {
        let picker = self
            .sticker_picker
            .as_ref()
            .filter(|picking| picking.open)?
            .picker
            .clone();
        Some(deferred(
            div()
                .absolute()
                .bottom(relative(1.0))
                .left_4()
                .mb_2()
                .occlude()
                .child(rise_in("sticker-picker-in", div().child(picker), 8.0)),
        ))
    }

    /// Whether `media`, a sticker, is among the favorites.
    fn is_favorite(&self, media: &MediaView) -> bool {
        self.favorite_stickers
            .contains(&Sticker::of(media.fetch.media.clone()).hash)
    }

    /// The menu entry keeping a sticker among favorites or taking it out,
    /// for a bubble carrying one.
    pub(super) fn with_favorite_sticker(
        &self,
        menu: Menu,
        bubble: &Bubble,
        cx: &mut Context<Self>,
    ) -> Menu {
        let Some(media) = bubble
            .media
            .clone()
            .filter(|media| media.fetch.kind == ContentKind::Sticker)
        else {
            return menu;
        };
        let favorite = self.is_favorite(&media);
        let view: WeakEntity<Self> = cx.weak_entity();
        menu.action(
            Icon::Sticker,
            if favorite {
                "Remove from favorites"
            } else {
                "Add to favorites"
            },
            move |_, cx| {
                let toggled = StickerFavoriteToggled {
                    sticker: Sticker::of(media.fetch.media.clone()),
                    file: media.file.clone(),
                    favorite: !favorite,
                };
                let _ = view.update(cx, |view, cx| {
                    view.close_message_popup(cx);
                    // Shown at once; the app keeps the favorites.
                    if toggled.favorite {
                        view.favorite_stickers.insert(toggled.sticker.hash.clone());
                    } else {
                        view.favorite_stickers.remove(&toggled.sticker.hash);
                    }
                    cx.emit(toggled);
                });
            },
        )
    }
}
