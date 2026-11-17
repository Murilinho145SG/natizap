//! What a media file holds, read without decoding it.

use std::path::Path;
use std::time::Duration;

use ffmpeg::media::Type;
use ffmpeg_next as ffmpeg;

use crate::MediaFileError;

/// A media file's length and picture size, as far as its container says.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Probe {
    /// How long it plays.
    pub duration: Option<Duration>,
    /// Picture width in pixels, when it has pictures.
    pub width: Option<u32>,
    /// Picture height in pixels, when it has pictures.
    pub height: Option<u32>,
}

/// Reads `path`'s container to tell how long it is and how big its
/// pictures are.
///
/// Blocks on file reads; run it off the UI thread.
///
/// # Errors
///
/// Fails when FFmpeg cannot open the file as media.
pub fn probe(path: &Path) -> Result<Probe, MediaFileError> {
    ffmpeg::init()?;
    let input = ffmpeg::format::input(path)?;
    let duration = u64::try_from(input.duration())
        .ok()
        .filter(|&micros| micros > 0)
        .map(Duration::from_micros);
    let picture = input
        .streams()
        .best(Type::Video)
        .and_then(|stream| {
            ffmpeg::codec::context::Context::from_parameters(stream.parameters())
                .ok()?
                .decoder()
                .video()
                .ok()
        })
        .map(|video| (video.width(), video.height()));
    Ok(Probe {
        duration,
        width: picture.map(|(width, _)| width),
        height: picture.map(|(_, height)| height),
    })
}
