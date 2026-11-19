use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{Div, FontWeight, ObjectFit, Pixels, div, img, px, rgb};

use crate::theme::{ACCENT, AVATAR_TINTS, DISPLAY_FONT, LINE, NAME_TINTS};

#[must_use]
/// `picture`, an [`avatar`], ringed as the person's status says: accent
/// while it holds something unseen, a hairline once all was seen, no ring
/// without a status.
pub fn status_ring(picture: Div, unseen: Option<bool>) -> Div {
    let ring = match unseen {
        Some(true) => rgb(ACCENT).into(),
        Some(false) => rgb(LINE).into(),
        None => gpui::transparent_black(),
    };
    div()
        .p(px(2.0))
        .rounded_full()
        .border_2()
        .border_color(ring)
        .child(picture)
}

/// A round avatar: `picture` when there is one, else the first letter of
/// `title` on a tint picked from `seed`, so a chat keeps its color wherever
/// it appears.
#[must_use]
pub fn avatar(seed: &str, title: &str, picture: Option<PathBuf>, size: Pixels) -> Div {
    if let Some(picture) = picture {
        return div()
            .size(size)
            .flex_none()
            .rounded_full()
            .bg(rgb(AVATAR_TINTS[tint_index(seed)]))
            // Rounded on the image itself: a parent's overflow clips to its
            // rectangle, not its rounded corners.
            .child(
                img(picture)
                    .size_full()
                    .rounded_full()
                    .object_fit(ObjectFit::Cover),
            );
    }
    let initial = title
        .chars()
        .find(|c| c.is_alphanumeric())
        .map_or_default(|c| c.to_uppercase().collect::<String>());
    div()
        .size(size)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(rgb(AVATAR_TINTS[tint_index(seed)]))
        .font_family(DISPLAY_FONT)
        .text_size(size * 0.4)
        .font_weight(FontWeight::SEMIBOLD)
        .child(initial)
}

/// Text color for a person's name in a group, stable per person.
#[must_use]
pub fn name_color(seed: &str) -> u32 {
    NAME_TINTS[tint_index(seed)]
}

fn tint_index(seed: &str) -> usize {
    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    usize::try_from(hasher.finish() % AVATAR_TINTS.len() as u64).unwrap_or_default()
}
