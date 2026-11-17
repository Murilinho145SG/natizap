//! Turning a file the user picked into something to send: what kind of
//! message it makes, and the preview, size and length WhatsApp shows
//! before it downloads.

use std::io::Cursor;
use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageFormat};
use natizap_playback::{probe, still_jpeg};
use natizap_protocol::{ContentKind, Outgoing};

/// Longest side of the preview sent along with photos and videos.
const PREVIEW_SIDE: u32 = 96;
/// WhatsApp's own ceiling for a single file.
const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Why a file cannot be sent.
#[derive(Debug, thiserror::Error)]
pub(crate) enum PrepareError {
    /// The file could not be read.
    #[error("could not read the file: {0}")]
    Read(#[from] std::io::Error),
    /// The file is larger than WhatsApp takes.
    #[error("the file is larger than 2 GB")]
    TooLarge,
    /// A photo could not be decoded or re-encoded.
    #[error("could not read the picture: {0}")]
    Picture(#[from] image::ImageError),
}

/// What kind of message a file makes, and its MIME type, by extension: as
/// the phone does, photos go as photos, MP4 as video, common sound files as
/// audio, and anything else as a document. Other video formats go as
/// documents because WhatsApp only plays MP4.
pub(crate) fn kind_of(path: &Path) -> (ContentKind, &'static str) {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map_or_default(str::to_ascii_lowercase);
    match extension.as_str() {
        "jpg" | "jpeg" => (ContentKind::Photo, "image/jpeg"),
        "png" => (ContentKind::Photo, "image/png"),
        "webp" => (ContentKind::Photo, "image/webp"),
        "mp4" | "m4v" => (ContentKind::Video, "video/mp4"),
        "mp3" => (ContentKind::Audio, "audio/mpeg"),
        "m4a" | "aac" => (ContentKind::Audio, "audio/mp4"),
        "ogg" | "opus" => (ContentKind::Audio, "audio/ogg; codecs=opus"),
        "pdf" => (ContentKind::Document, "application/pdf"),
        "txt" => (ContentKind::Document, "text/plain"),
        "zip" => (ContentKind::Document, "application/zip"),
        "doc" => (ContentKind::Document, "application/msword"),
        "docx" => (
            ContentKind::Document,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        ),
        "xls" => (ContentKind::Document, "application/vnd.ms-excel"),
        "xlsx" => (
            ContentKind::Document,
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        ),
        "pptx" => (
            ContentKind::Document,
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        ),
        _ => (ContentKind::Document, "application/octet-stream"),
    }
}

/// Reads `path` and makes it ready to send with `caption`, as a document
/// when `as_document`, whatever it is, so pictures keep their quality.
///
/// Blocks on file reads and decoding; run it off the UI thread.
///
/// # Errors
///
/// Fails when the file cannot be read, is too large, or is a photo that
/// cannot be decoded.
pub(crate) fn prepare(
    path: &Path,
    caption: Option<String>,
    as_document: bool,
) -> Result<Outgoing, PrepareError> {
    if std::fs::metadata(path)?.len() > MAX_BYTES {
        return Err(PrepareError::TooLarge);
    }
    let (kind, mime) = kind_of(path);
    let kind = if as_document {
        ContentKind::Document
    } else {
        kind
    };
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    let mut outgoing = Outgoing {
        kind,
        bytes: Vec::new(),
        mime: mime.to_owned(),
        file_name: None,
        caption,
        thumbnail: None,
        width: None,
        height: None,
        seconds: None,
        waveform: None,
    };
    match kind {
        ContentKind::Photo => {
            let bytes = std::fs::read(path)?;
            let picture = image::load_from_memory(&bytes)?;
            outgoing.width = Some(picture.width());
            outgoing.height = Some(picture.height());
            outgoing.thumbnail = Some(jpeg(&picture.thumbnail(PREVIEW_SIDE, PREVIEW_SIDE), 70)?);
            // WhatsApp shows photos as JPEG; others are converted, as the
            // phone does.
            "image/jpeg".clone_into(&mut outgoing.mime);
            outgoing.bytes = if image::guess_format(&bytes).ok() == Some(ImageFormat::Jpeg) {
                bytes
            } else {
                jpeg(&picture, 90)?
            };
        }
        ContentKind::Video | ContentKind::Audio => {
            outgoing.bytes = std::fs::read(path)?;
            // A file FFmpeg cannot read still sends; the other side just
            // learns its length when it plays.
            if let Ok(found) = probe(path) {
                outgoing.seconds = found
                    .duration
                    .and_then(|duration| u32::try_from(duration.as_secs()).ok());
                outgoing.width = found.width;
                outgoing.height = found.height;
            }
            if kind == ContentKind::Video {
                outgoing.thumbnail = still_jpeg(path, PREVIEW_SIDE).ok();
            }
        }
        _ => {
            outgoing.bytes = std::fs::read(path)?;
            outgoing.file_name = file_name;
        }
    }
    Ok(outgoing)
}

fn jpeg(picture: &DynamicImage, quality: u8) -> Result<Vec<u8>, image::ImageError> {
    let mut encoded = Cursor::new(Vec::new());
    JpegEncoder::new_with_quality(&mut encoded, quality).encode_image(&picture.to_rgb8())?;
    Ok(encoded.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn picks_the_message_kind_from_the_extension() {
        let kind = |name: &str| kind_of(Path::new(name)).0;
        assert_eq!(kind("beach.JPG"), ContentKind::Photo);
        assert_eq!(kind("screen.png"), ContentKind::Photo);
        assert_eq!(kind("clip.mp4"), ContentKind::Video);
        assert_eq!(kind("clip.mkv"), ContentKind::Document);
        assert_eq!(kind("song.mp3"), ContentKind::Audio);
        assert_eq!(kind("notes.pdf"), ContentKind::Document);
        assert_eq!(kind("no-extension"), ContentKind::Document);
    }

    #[test]
    fn sends_a_png_as_a_jpeg_photo_with_a_preview() -> TestResult {
        let dir = std::env::temp_dir().join("natizap-outbox");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("shot.png");
        image::RgbImage::from_pixel(400, 200, image::Rgb([200, 80, 20])).save(&path)?;

        let outgoing = prepare(&path, Some("look".into()), false)?;
        assert_eq!(outgoing.kind, ContentKind::Photo);
        assert_eq!(outgoing.mime, "image/jpeg");
        assert_eq!(image::guess_format(&outgoing.bytes)?, ImageFormat::Jpeg);
        assert_eq!((outgoing.width, outgoing.height), (Some(400), Some(200)));
        let preview = image::load_from_memory(&outgoing.thumbnail.ok_or("no preview")?)?;
        assert_eq!((preview.width(), preview.height()), (96, 48));
        assert_eq!(outgoing.caption.as_deref(), Some("look"));
        Ok(())
    }

    #[test]
    fn documents_keep_their_name() -> TestResult {
        let dir = std::env::temp_dir().join("natizap-outbox");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("Nota fiscal.pdf");
        std::fs::write(&path, b"%PDF-1.4")?;
        let outgoing = prepare(&path, None, false)?;
        assert_eq!(outgoing.kind, ContentKind::Document);
        assert_eq!(outgoing.file_name.as_deref(), Some("Nota fiscal.pdf"));
        assert_eq!(outgoing.bytes, b"%PDF-1.4");
        Ok(())
    }

    #[test]
    fn a_picture_sent_as_a_document_stays_untouched() -> TestResult {
        let dir = std::env::temp_dir().join("natizap-outbox");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("scan.png");
        image::RgbImage::from_pixel(40, 20, image::Rgb([1, 2, 3])).save(&path)?;
        let outgoing = prepare(&path, None, true)?;
        assert_eq!(outgoing.kind, ContentKind::Document);
        assert_eq!(outgoing.mime, "image/png");
        assert_eq!(outgoing.bytes, std::fs::read(&path)?);
        Ok(())
    }
}
