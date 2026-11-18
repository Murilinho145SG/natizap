//! Turning a chosen photo into the square JPEG WhatsApp takes as a
//! profile picture.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;

/// The side WhatsApp's apps upload profile pictures at.
const SIDE: u32 = 640;

/// `bytes`, a photo of any shape, cut to its centred square and scaled to
/// [`SIDE`], as JPEG. Blocks while it decodes; run it off the UI thread.
///
/// # Errors
///
/// Fails when `bytes` is not a picture `image` can read.
pub(crate) fn square_jpeg(bytes: &[u8]) -> image::ImageResult<Vec<u8>> {
    let picture = image::load_from_memory(bytes)?;
    let side = picture.width().min(picture.height());
    let square = picture.crop_imm(
        (picture.width() - side) / 2,
        (picture.height() - side) / 2,
        side,
        side,
    );
    let scaled = square
        .resize_exact(SIDE, SIDE, FilterType::Lanczos3)
        .to_rgb8();
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut Cursor::new(&mut jpeg), 88).encode_image(&scaled)?;
    Ok(jpeg)
}

#[cfg(test)]
mod tests {
    use image::{ImageFormat, Rgb, RgbImage};

    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        // Left half red, right half blue: the centre square holds both.
        let picture = RgbImage::from_fn(width, height, |x, _| {
            if x < width / 2 {
                Rgb([255, 0, 0])
            } else {
                Rgb([0, 0, 255])
            }
        });
        let mut bytes = Vec::new();
        picture
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap_or_else(|error| panic!("could not write a test picture: {error}"));
        bytes
    }

    #[test]
    fn cuts_a_wide_photo_to_a_square_jpeg_of_the_upload_size() {
        let jpeg = square_jpeg(&png(300, 200)).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(image::guess_format(&jpeg).ok(), Some(ImageFormat::Jpeg));
        let decoded = image::load_from_memory(&jpeg).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!((decoded.width(), decoded.height()), (SIDE, SIDE));
        let decoded = decoded.to_rgb8();
        // Cut at the centre, so red stays left and blue right.
        assert!(decoded.get_pixel(10, SIDE / 2)[0] > 200);
        assert!(decoded.get_pixel(SIDE - 10, SIDE / 2)[2] > 200);
    }

    #[test]
    fn refuses_what_is_not_a_picture() {
        assert!(square_jpeg(b"not a picture").is_err());
    }
}
