//! A still from a video, to show in place of the video until it plays.

use std::io::Cursor;
use std::path::Path;
use std::time::Duration;

use ffmpeg::codec;
use ffmpeg::format::Pixel;
use ffmpeg::media::Type;
use ffmpeg::software::scaling;
use ffmpeg::util::frame;
use ffmpeg_next as ffmpeg;
use image::codecs::jpeg::JpegEncoder;

use crate::decode::{fit, packed_rows, seconds};
use crate::turn::Turn;

/// Widest poster worth keeping: a bubble never shows one wider.
const POSTER_WIDTH: u32 = 640;
/// Where the still is taken from, in microseconds. Videos often open on a
/// black or fading frame, so a second in looks more like the video.
const STILL_AT: i64 = 1_000_000;

/// Why a media file could not be read, or a still of it made.
#[derive(Debug, thiserror::Error)]
pub enum MediaFileError {
    /// FFmpeg could not open or decode the file.
    #[error("{0}")]
    Media(#[from] ffmpeg::Error),
    /// The file has no picture to take a still from.
    #[error("the file has no video")]
    NoVideo,
    /// The still could not be encoded.
    #[error("could not encode the still: {0}")]
    Encode(#[from] image::ImageError),
    /// The still could not be written.
    #[error("could not write the still: {0}")]
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
pub fn save_poster(video: &Path, poster: &Path) -> Result<(), MediaFileError> {
    let jpeg = still_jpeg(video, POSTER_WIDTH)?;
    // Written aside and renamed, so a half-written poster never shows.
    let partial = poster.with_extension("part");
    std::fs::write(&partial, jpeg)?;
    std::fs::rename(&partial, poster)?;
    Ok(())
}

/// A JPEG still of `video`, at most `max_width` pixels wide, e.g. the
/// small preview sent along with a video.
///
/// Blocks while it decodes; run it off the UI thread.
///
/// # Errors
///
/// Fails when the video cannot be decoded or has no picture.
pub fn still_jpeg(video: &Path, max_width: u32) -> Result<Vec<u8>, MediaFileError> {
    ffmpeg::init()?;
    let mut input = ffmpeg::format::input(video)?;
    let still_at = if input.duration() > 2 * STILL_AT {
        input.seek(STILL_AT, ..STILL_AT)?;
        Duration::from_micros(STILL_AT.unsigned_abs())
    } else {
        Duration::ZERO
    };
    let (still, turn) = decode_still(&mut input, still_at)?;

    let (width, height) = fit(still.width(), still.height(), max_width);
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
    let packed = packed_rows(&rgb, width as usize * 3, height as usize);
    let (pixels, width, height) = turn.apply(packed, width, height, 3);
    let mut jpeg = Cursor::new(Vec::new());
    JpegEncoder::new_with_quality(&mut jpeg, 85).encode(
        &pixels,
        width,
        height,
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(jpeg.into_inner())
}

/// The first picture at or after `still_at`, or the last one if the video
/// ends before it, and how to turn it upright.
fn decode_still(
    input: &mut ffmpeg::format::context::Input,
    still_at: Duration,
) -> Result<(frame::Video, Turn), MediaFileError> {
    let (index, time_base, parameters, turn) = {
        let stream = input
            .streams()
            .best(Type::Video)
            .ok_or(MediaFileError::NoVideo)?;
        (
            stream.index(),
            stream.time_base(),
            stream.parameters(),
            Turn::of(&stream),
        )
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
                return Ok((still, turn));
            }
        }
    }
    decoder.send_eof()?;
    while decoder.receive_frame(&mut still).is_ok() {
        found = true;
    }
    if found {
        Ok((still, turn))
    } else {
        Err(MediaFileError::NoVideo)
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
        assert!(matches!(result, Err(MediaFileError::Media(_))));
        assert!(!poster.exists());
    }
}
