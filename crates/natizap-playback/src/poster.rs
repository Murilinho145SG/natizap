//! A still from a video, to show in place of the video until it plays.

use std::path::Path;
use std::time::Duration;

use ffmpeg::codec;
use ffmpeg::format::Pixel;
use ffmpeg::media::Type;
use ffmpeg::software::scaling;
use ffmpeg::util::frame;
use ffmpeg_next as ffmpeg;

use crate::decode::{fit, packed_rows, seconds};

/// Widest poster worth keeping: a bubble never shows one wider.
const POSTER_WIDTH: u32 = 640;
/// Where the still is taken from, in microseconds. Videos often open on a
/// black or fading frame, so a second in looks more like the video.
const STILL_AT: i64 = 1_000_000;

/// Why a poster could not be made.
#[derive(Debug, thiserror::Error)]
pub enum PosterError {
    /// FFmpeg could not open or decode the video.
    #[error("{0}")]
    Media(#[from] ffmpeg::Error),
    /// The file has no picture to take a still from.
    #[error("the file has no video")]
    NoVideo,
    /// The still could not be encoded.
    #[error("could not encode the poster: {0}")]
    Encode(#[from] image::ImageError),
    /// The poster could not be written.
    #[error("could not write the poster: {0}")]
    Write(#[from] std::io::Error),
}

/// Writes a JPEG still of `video` to `poster`, at most 640 pixels wide.
///
/// Blocks while it decodes; run it off the UI thread.
///
/// # Errors
///
/// Fails when the video cannot be decoded or has no picture, or the poster
/// cannot be written. A failed attempt leaves no file at `poster`.
pub fn save_poster(video: &Path, poster: &Path) -> Result<(), PosterError> {
    ffmpeg::init()?;
    let mut input = ffmpeg::format::input(video)?;
    let still_at = if input.duration() > 2 * STILL_AT {
        input.seek(STILL_AT, ..STILL_AT)?;
        Duration::from_micros(STILL_AT.unsigned_abs())
    } else {
        Duration::ZERO
    };
    let still = decode_still(&mut input, still_at)?;

    let (width, height) = fit(still.width(), still.height(), POSTER_WIDTH);
    let mut scaler = scaling::Context::get(
        still.format(),
        still.width(),
        still.height(),
        Pixel::RGB24,
        width,
        height,
        scaling::Flags::BILINEAR,
    )?;
    let mut rgb = frame::Video::empty();
    scaler.run(&still, &mut rgb)?;
    let pixels = packed_rows(&rgb, width as usize * 3, height as usize);

    // Written aside and renamed, so a half-written poster never shows.
    let partial = poster.with_extension("part");
    image::save_buffer_with_format(
        &partial,
        &pixels,
        width,
        height,
        image::ExtendedColorType::Rgb8,
        image::ImageFormat::Jpeg,
    )?;
    std::fs::rename(&partial, poster)?;
    Ok(())
}

/// The first picture at or after `still_at`, or the last one if the video
/// ends before it.
fn decode_still(
    input: &mut ffmpeg::format::context::Input,
    still_at: Duration,
) -> Result<frame::Video, PosterError> {
    let (index, time_base, parameters) = {
        let stream = input
            .streams()
            .best(Type::Video)
            .ok_or(PosterError::NoVideo)?;
        (stream.index(), stream.time_base(), stream.parameters())
    };
    let mut decoder = codec::context::Context::from_parameters(parameters)?
        .decoder()
        .video()?;
    let mut still = frame::Video::empty();
    let mut found = false;
    for (stream, packet) in input.packets() {
        if stream.index() != index {
            continue;
        }
        decoder.send_packet(&packet)?;
        // Seeking lands on the keyframe before the still, which may be the
        // very first, dark frame; decode on up to the still itself.
        while decoder.receive_frame(&mut still).is_ok() {
            found = true;
            if seconds(still.timestamp(), time_base) >= still_at {
                return Ok(still);
            }
        }
    }
    decoder.send_eof()?;
    while decoder.receive_frame(&mut still).is_ok() {
        found = true;
    }
    if found {
        Ok(still)
    } else {
        Err(PosterError::NoVideo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_video_leaves_no_poster() {
        let dir = std::env::temp_dir().join("natizap-poster-test");
        let poster = dir.join("missing.jpg");
        let result = save_poster(&dir.join("missing.mp4"), &poster);
        assert!(matches!(result, Err(PosterError::Media(_))));
        assert!(!poster.exists());
    }
}
