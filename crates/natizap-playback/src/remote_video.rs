//! The other end's video in a call: H.264 access units decoded on a
//! thread of their own into pictures, turned upright, for the call's
//! view to draw the newest of.

use ffmpeg::codec;
use ffmpeg::format::Pixel;
use ffmpeg::software::scaling;
use ffmpeg::util::frame;
use ffmpeg_next as ffmpeg;

use crate::Picture;
use crate::decode::{fit, packed_rows};
use crate::feed::{FeedWriter, VideoFeed, feed};
use crate::turn::Turn;

/// The widest a call's picture is drawn; a phone sends at most 720p.
const MAX_WIDTH: u32 = 1280;
/// Access units waiting for the decoder before new ones are dropped.
const BACKLOG: usize = 8;

/// Why a call's video cannot start.
#[derive(Debug, thiserror::Error)]
pub enum VideoError {
    /// There is no camera, or it would not open.
    #[error("the camera could not be opened: {0}")]
    Camera(String),
    /// A video thread could not start.
    #[error("could not start the call's video: {0}")]
    Thread(String),
}

/// Where a call's received video goes: each H.264 access unit, in
/// Annex-B, with how many quarter turns clockwise the sending phone was
/// held.
pub type AccessUnits = async_channel::Sender<(Vec<u8>, u8)>;

/// Starts decoding the other end's video. Send it access units in
/// Annex-B, with how many quarter turns clockwise the sending phone was
/// held; it decodes until every sender is dropped.
///
/// # Errors
///
/// Fails when the decoding thread cannot start.
pub fn decode_video() -> Result<(VideoFeed, AccessUnits), VideoError> {
    let (units, received) = async_channel::bounded(BACKLOG);
    let (writer, feed) = feed();
    // A plain thread: FFmpeg blocks, and must not share a pool with UI
    // or protocol work.
    std::thread::Builder::new()
        .name("natizap-call-video".into())
        .spawn(move || decode(&received, &writer))
        .map_err(|error| VideoError::Thread(error.to_string()))?;
    Ok((feed, units))
}

fn decode(units: &async_channel::Receiver<(Vec<u8>, u8)>, writer: &FeedWriter) {
    let mut decoder = match Decoder::new() {
        Ok(decoder) => decoder,
        Err(error) => {
            tracing::warn!(%error, "no H.264 decoder: the call's video stays dark");
            return;
        }
    };
    while let Ok((unit, quarters)) = units.recv_blocking() {
        let turn = upright(quarters);
        match decoder.decode(&unit, turn) {
            Ok(Some(picture)) => writer.show(picture),
            Ok(None) => {}
            // A damaged unit shows as a frozen picture until the next
            // keyframe, as on a bad network.
            Err(error) => tracing::debug!(%error, "dropped a video access unit"),
        }
    }
}

/// The turn that shows upright a picture from a phone held `quarters`
/// quarter turns from upright: WhatsApp counts the phone's turns
/// clockwise, so the picture turns back the other way.
fn upright(quarters: u8) -> Turn {
    Turn::nearest(-f64::from(quarters) * 90.0)
}

/// FFmpeg's H.264 decoder and the scaler to BGRA it needs for the size it
/// decodes at.
pub(crate) struct Decoder {
    h264: ffmpeg::decoder::Video,
    decoded: frame::Video,
    scaler: Option<(Shape, scaling::Context)>,
}

/// What a scaler converts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shape {
    format: Pixel,
    width: u32,
    height: u32,
}

impl Decoder {
    pub(crate) fn new() -> Result<Self, ffmpeg::Error> {
        ffmpeg::init()?;
        let codec = ffmpeg::decoder::find(codec::Id::H264).ok_or(ffmpeg::Error::DecoderNotFound)?;
        let h264 = codec::context::Context::new_with_codec(codec)
            .decoder()
            .video()?;
        Ok(Self {
            h264,
            decoded: frame::Video::empty(),
            scaler: None,
        })
    }

    /// The newest picture `unit` completes, turned by `turn`.
    pub(crate) fn decode(
        &mut self,
        unit: &[u8],
        turn: Turn,
    ) -> Result<Option<Picture>, ffmpeg::Error> {
        self.h264.send_packet(&ffmpeg::Packet::copy(unit))?;
        let mut picture = None;
        while self.h264.receive_frame(&mut self.decoded).is_ok() {
            let shape = Shape {
                format: self.decoded.format(),
                width: self.decoded.width(),
                height: self.decoded.height(),
            };
            let (width, height) = fit(shape.width, shape.height, MAX_WIDTH);
            let scaler = match &mut self.scaler {
                Some((made_for, scaler)) if *made_for == shape => scaler,
                slot => {
                    let scaler = scaling::Context::get(
                        shape.format,
                        shape.width,
                        shape.height,
                        Pixel::BGRA,
                        width,
                        height,
                        scaling::Flags::BILINEAR,
                    )?;
                    &mut slot.insert((shape, scaler)).1
                }
            };
            let mut bgra = frame::Video::empty();
            scaler.run(&self.decoded, &mut bgra)?;
            let packed = packed_rows(&bgra, width as usize * 4, height as usize);
            let (bgra, width, height) = turn.apply(packed, width, height, 4);
            picture = Some(Picture {
                width,
                height,
                bgra,
            });
        }
        Ok(picture)
    }
}

#[cfg(test)]
mod tests {
    use super::upright;
    use crate::turn::Turn;

    #[test]
    fn a_phone_turned_one_way_is_turned_back_the_other() {
        // A phone held upright sends orientation 1 with sideways pictures.
        assert_eq!(upright(0), Turn::None);
        assert_eq!(upright(1), Turn::Counterclockwise);
        assert_eq!(upright(2), Turn::Half);
        assert_eq!(upright(3), Turn::Clockwise);
    }
}
