//! Small copies of downloaded photos and video stills, for grids of many:
//! drawing the originals would decode every one at full size.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;

/// The longest side of a small copy: sharp in a grid tile at twice its
/// size, for high-density screens.
const SIDE: u32 = 256;

/// Where message `message_id`'s small copy is kept inside the media
/// folder `media_dir`.
pub(crate) fn path_in(media_dir: &Path, message_id: &str) -> PathBuf {
    let id: String = message_id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    media_dir.join("thumbs").join(format!("{id}.jpg"))
}

/// Writes a small copy of the picture at `original` to `small`. Blocks
/// while it decodes; run it off the UI thread.
///
/// # Errors
///
/// Fails when the picture cannot be read or the copy cannot be written. A
/// failed attempt leaves no file at `small`.
pub(crate) fn make(original: &Path, small: &Path) -> image::ImageResult<()> {
    let picture = image::open(original)?.thumbnail(SIDE, SIDE).to_rgb8();
    if let Some(dir) = small.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Written aside and renamed, so a half-written copy never shows.
    let partial = small.with_extension("part");
    {
        let mut out = BufWriter::new(File::create(&partial)?);
        JpegEncoder::new_with_quality(&mut out, 82).encode_image(&picture)?;
    }
    std::fs::rename(&partial, small)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{SIDE, make, path_in};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn makes_a_small_copy_keeping_the_shape() -> TestResult {
        let dir = std::env::temp_dir().join(format!("natizap-thumbs-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let original = dir.join("tall.png");
        image::RgbImage::from_pixel(600, 1200, image::Rgb([200, 120, 30])).save(&original)?;
        let small = path_in(&dir, "3EB0:ABC/1");
        make(&original, &small)?;
        let made = image::open(&small)?;
        let result = (made.width(), made.height());
        std::fs::remove_dir_all(&dir)?;
        assert_eq!(result, (SIDE / 2, SIDE));
        Ok(())
    }

    #[test]
    fn keeps_copies_under_safe_names() {
        let path = path_in(std::path::Path::new("/media"), "3EB0:ABC/../x");
        assert_eq!(path, std::path::Path::new("/media/thumbs/3EB0ABCx.jpg"));
    }
}
