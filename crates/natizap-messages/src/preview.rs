//! The inline preview WhatsApp sends with photos and videos, made fit to
//! show at bubble size.

use std::io::Cursor;
use std::sync::Arc;

use gpui::{Image, ImageFormat};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::{self, FilterType};
use natizap_protocol::ContentKind;

/// Width the preview is enlarged to before softening; the GPU stretches it
/// the rest of the way, which a soft picture survives.
const SOFT_WIDTH: u32 = 160;
/// How much to soften, in pixels at [`SOFT_WIDTH`].
const SOFTNESS: f32 = 3.5;

/// The preview `bytes` of a `kind` attachment, ready to draw.
///
/// Photo and video previews are JPEGs a few dozen pixels wide that turn to
/// blocks when stretched to bubble size, so they are enlarged and softened
/// first, as the phone shows them. Stickers keep their own PNG. The result
/// is encoded rather than raw pixels: GPUI keeps one copy per distinct
/// image, so redrawing the chat does not pile up copies.
pub(crate) fn preview(kind: ContentKind, bytes: Vec<u8>) -> Arc<Image> {
    if bytes.starts_with(b"\x89PNG") {
        return Arc::new(Image::from_bytes(ImageFormat::Png, bytes));
    }
    let soft = matches!(kind, ContentKind::Photo | ContentKind::Video)
        .then(|| soften(&bytes))
        .flatten();
    Arc::new(Image::from_bytes(ImageFormat::Jpeg, soft.unwrap_or(bytes)))
}

/// `None` when the bytes are not a JPEG this can read; the caller then
/// shows them as they came.
fn soften(jpeg: &[u8]) -> Option<Vec<u8>> {
    let small = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg)
        .ok()?
        .to_rgb8();
    let (width, height) = small.dimensions();
    if width == 0 || width >= SOFT_WIDTH {
        return None;
    }
    let tall = u64::from(height) * u64::from(SOFT_WIDTH) / u64::from(width);
    let enlarged = imageops::resize(
        &small,
        SOFT_WIDTH,
        u32::try_from(tall).ok()?.max(1),
        FilterType::Triangle,
    );
    let soft = imageops::fast_blur(&enlarged, SOFTNESS);
    let mut encoded = Cursor::new(Vec::new());
    JpegEncoder::new_with_quality(&mut encoded, 85)
        .encode_image(&soft)
        .ok()?;
    Some(encoded.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn tiny_jpeg() -> image::ImageResult<Vec<u8>> {
        let pixels = image::RgbImage::from_fn(32, 18, |x, y| {
            image::Rgb([
                u8::try_from(x * 8).unwrap_or(0),
                u8::try_from(y * 14).unwrap_or(0),
                90,
            ])
        });
        let mut encoded = Cursor::new(Vec::new());
        JpegEncoder::new(&mut encoded).encode_image(&pixels)?;
        Ok(encoded.into_inner())
    }

    #[test]
    fn enlarges_photo_and_video_previews_keeping_their_shape() -> TestResult {
        let soft = soften(&tiny_jpeg()?).ok_or("a valid JPEG did not soften")?;
        let decoded = image::load_from_memory(&soft)?;
        assert_eq!((decoded.width(), decoded.height()), (160, 90));
        Ok(())
    }

    #[test]
    fn leaves_what_it_cannot_read_as_it_came() {
        let bytes = b"not a jpeg".to_vec();
        assert_eq!(preview(ContentKind::Video, bytes.clone()).bytes(), bytes);
    }
}
