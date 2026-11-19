//! natizap's icon in a theme's colours: the mark in the accent on a tile
//! of the deepest surface.

use crate::theme::Theme;

/// The tile's and the mark's colours in the bundled brand icon, as
/// `brand/natizap-icon.svg` draws them.
const BRAND_TILE: u32 = 0x0f_11_14;
const BRAND_MARK: u32 = 0xff_7a_1a;

/// natizap's icon drawn in `theme`, as `side`×`side` RGBA pixels.
#[must_use]
pub fn app_icon(theme: Theme, side: u32) -> Vec<u8> {
    let brand = match image::load_from_memory_with_format(
        include_bytes!("../../../brand/natizap-icon.png"),
        image::ImageFormat::Png,
    ) {
        Ok(brand) => brand,
        Err(error) => {
            // Bundled at build time, so this cannot happen with a sound
            // build; a blank icon still marks the place.
            tracing::error!(%error, "could not read the bundled app icon");
            return vec![0; (side * side * 4) as usize];
        }
    };
    let mut pixels = brand
        .resize_exact(side, side, image::imageops::FilterType::Lanczos3)
        .into_rgba8()
        .into_raw();
    let (tile, mark) = (channels(theme.deep), channels(theme.accent));
    let (from, to) = (channels(BRAND_TILE), channels(BRAND_MARK));
    // The brand icon holds only its two colours and the blend between them
    // along its edges; red tells them apart most, so it measures how much
    // of the mark each pixel shows.
    let span = f32::from(to[0]) - f32::from(from[0]);
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let share = ((f32::from(pixel[0]) - f32::from(from[0])) / span).clamp(0.0, 1.0);
        for channel in 0..3 {
            let blended = f32::from(tile[channel])
                + (f32::from(mark[channel]) - f32::from(tile[channel])) * share;
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a blend of two channels stays within 0..=255"
            )]
            let blended = blended.round() as u8;
            pixel[channel] = blended;
        }
    }
    pixels
}

/// The red, green and blue of `0xRRGGBB`.
fn channels(color: u32) -> [u8; 3] {
    let [_, red, green, blue] = color.to_be_bytes();
    [red, green, blue]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::NATIVE;

    fn at(pixels: &[u8], side: u32, x: u32, y: u32) -> [u8; 4] {
        let start = ((y * side + x) * 4) as usize;
        [
            pixels[start],
            pixels[start + 1],
            pixels[start + 2],
            pixels[start + 3],
        ]
    }

    #[test]
    fn draws_the_tile_and_the_mark_in_the_theme() {
        let side = 128;
        for (name, theme) in NATIVE {
            let pixels = app_icon(theme, side);
            assert_eq!(pixels.len(), (side * side * 4) as usize, "{name}");
            let [r, g, b] = channels(theme.deep);
            // Inside the tile, left of the mark.
            assert_eq!(
                at(&pixels, side, 16, side / 2),
                [r, g, b, 255],
                "{name}: tile"
            );
            let [r, g, b] = channels(theme.accent);
            // On the mark's left stem.
            assert_eq!(
                at(&pixels, side, 40, side / 2),
                [r, g, b, 255],
                "{name}: mark"
            );
            // The rounded corner stays see-through.
            assert_eq!(at(&pixels, side, 0, 0)[3], 0, "{name}: corner");
        }
    }
}
