//! Stickers between features: the favorites the stickers crate keeps, the
//! picker it draws inside the conversation, and the stickers people send
//! that the conversation offers to keep.

use gpui::prelude::*;
use gpui::{Context, Entity};
use natizap_messages::{ConversationView, MessageLog, StickerFavoriteToggled};
use natizap_stickers::{StickerPicked, StickerPicker, StickerShelf};

use super::Root;

impl Root {
    /// Hands the conversation the sticker picker, sends what is picked,
    /// tells it which stickers are favorites and keeps the ones it asks to.
    pub(super) fn share_stickers(
        shelf: &Entity<StickerShelf>,
        conversation: &Entity<ConversationView>,
        messages: &Entity<MessageLog>,
        cx: &mut Context<Self>,
    ) {
        let picker = cx.new(|cx| StickerPicker::new(shelf.clone(), cx));
        conversation.update(cx, |conversation, _| {
            conversation.set_sticker_picker(picker.clone().into());
        });

        let (sending, closing, files) = (messages.clone(), conversation.clone(), shelf.clone());
        cx.subscribe(&picker, move |_, _, StickerPicked(favorite), cx| {
            let favorite = favorite.clone();
            // A sticker still downloading shows from where it will land.
            let file = favorite
                .file
                .unwrap_or_else(|| files.read(cx).file_for(&favorite.sticker));
            sending.update(cx, |log, cx| {
                log.send_sticker(favorite.sticker, Some(file), cx);
            });
            closing.update(cx, ConversationView::close_sticker_picker);
        })
        .detach();

        let telling = conversation.clone();
        cx.observe(shelf, move |_, shelf, cx| {
            let hashes = shelf.read(cx).hashes();
            telling.update(cx, |conversation, cx| {
                conversation.set_favorite_stickers(hashes, cx);
            });
        })
        .detach();

        let keeping = shelf.clone();
        cx.subscribe(
            conversation,
            move |_, _, toggled: &StickerFavoriteToggled, cx| {
                let toggled = toggled.clone();
                keeping.update(cx, |shelf, cx| {
                    shelf.set_favorite(toggled.sticker, toggled.file, toggled.favorite, cx);
                });
            },
        )
        .detach();
    }
}
