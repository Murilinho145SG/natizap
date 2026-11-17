//! Color emoji drawn as images.
//!
//! GPUI's Linux text system only takes fallback fonts that contain Latin
//! letters, which rules out emoji fonts, so emoji in plain text render
//! monochrome. Where emoji stand alone (pickers, reactions) they are drawn
//! from bundled Twemoji graphics instead.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use gpui::prelude::*;
use gpui::{AnyElement, Image, ImageFormat, Pixels, div, img};

use crate::emoji_table::EMOJI;

/// `emoji` at `size`: in color when it is bundled, as text otherwise.
#[must_use]
pub fn emoji(emoji: &str, size: Pixels) -> AnyElement {
    match image(emoji) {
        Some(image) => img(image).size(size).flex_none().into_any_element(),
        None => div()
            .text_size(size * 0.85)
            .child(emoji.to_owned())
            .into_any_element(),
    }
}

fn image(emoji: &str) -> Option<Arc<Image>> {
    static IMAGES: OnceLock<HashMap<&'static str, Arc<Image>>> = OnceLock::new();
    let images = IMAGES.get_or_init(|| {
        EMOJI
            .iter()
            .map(|&(name, svg)| {
                (
                    name,
                    Arc::new(Image::from_bytes(ImageFormat::Svg, svg.to_vec())),
                )
            })
            .collect()
    });
    images.get(file_name(emoji).as_str()).cloned()
}

/// Twemoji's file name for `emoji`: code points in hex, dropping the emoji
/// presentation selector unless the sequence joins several emoji.
fn file_name(emoji: &str) -> String {
    let joined = emoji.contains('\u{200d}');
    emoji
        .chars()
        .filter(|&c| joined || c != '\u{fe0f}')
        .map(|c| format!("{:x}", u32::from(c)))
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_bundled_emoji_with_and_without_presentation_selector() {
        assert_eq!(file_name("❤️"), "2764");
        assert!(image("❤️").is_some());
        assert!(image("😂").is_some());
        assert!(image("x").is_none());
    }
}
