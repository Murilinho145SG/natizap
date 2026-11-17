use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{Div, FontWeight, ObjectFit, Pixels, div, img, rgb};

use crate::theme::{AVATAR_TINTS, DISPLAY_FONT, NAME_TINTS};

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
            .overflow_hidden()
            .bg(rgb(AVATAR_TINTS[tint_index(seed)]))
            .child(img(picture).size_full().object_fit(ObjectFit::Cover));
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
