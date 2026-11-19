//! A call's camera: captured through FFmpeg's own device inputs (V4L2 on
//! Linux, `AVFoundation` on macOS, `DirectShow` on Windows), scaled, encoded
//! as H.264 for the call, and shown small, mirrored, as the user sees
//! themselves.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ffmpeg::format::Pixel;
use ffmpeg::media::Type;
use ffmpeg::software::scaling;
use ffmpeg::util::frame;
use ffmpeg::{Dictionary, codec};
use ffmpeg_next as ffmpeg;

use crate::Picture;
use crate::decode::{fit, packed_rows};
use crate::devices::default_camera;
use crate::feed::{FeedWriter, VideoFeed, feed};
use crate::h264::{FRAME_RATE, H264Encoder, Source};
use crate::inset::Inset;
use crate::remote_video::VideoError;

/// The widest the user's own picture is drawn: it shows small.
const PREVIEW_WIDTH: u32 = 320;
/// Access units waiting for the call before new ones are dropped.
const BACKLOG: usize = 4;

/// The camera, open for a call until dropped.
#[derive(Debug)]
pub struct Camera {
    stop: Arc<AtomicBool>,
    inset: Inset,
    keyframe: Arc<AtomicBool>,
    preview: VideoFeed,
}

impl Drop for Camera {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.inset.clear();
    }
}

impl Camera {
    /// Opens `device`, or the default camera. The receiver gives the call
    /// one Annex-B access unit per picture, fifteen a second, except while
    /// `inset` lends the pictures to the shared screen. Blocks while the
    /// camera opens; run it off the UI thread.
    ///
    /// # Errors
    ///
    /// Fails when there is no camera or it will not open.
    pub fn start(
        device: Option<&str>,
        inset: Inset,
    ) -> Result<(Self, async_channel::Receiver<Vec<u8>>), VideoError> {
        let path = device.unwrap_or_else(|| default_camera()).to_owned();
        Self::open((input_format(), path), inset)
    }

    /// Opens the device `input` names: an FFmpeg input format and what
    /// it opens.
    fn open(
        input: (&'static str, String),
        inset: Inset,
    ) -> Result<(Self, async_channel::Receiver<Vec<u8>>), VideoError> {
        let (units, received) = async_channel::bounded(BACKLOG);
        let (writer, preview) = feed();
        let (stop, keyframe) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(true)),
        );
        let (opened_tx, opened) = std::sync::mpsc::channel();
        let (stopping, wanting) = (Arc::clone(&stop), Arc::clone(&keyframe));
        let lending = inset.clone();
        // A plain thread: reading the camera blocks.
        std::thread::Builder::new()
            .name("natizap-camera".into())
            .spawn(move || match Capture::open(input) {
                Ok(capture) => {
                    let _ = opened_tx.send(Ok(()));
                    capture.run(&units, &writer, &lending, (&stopping, &wanting));
                }
                Err(error) => {
                    let _ = opened_tx.send(Err(error));
                }
            })
            .map_err(|error| VideoError::Thread(error.to_string()))?;
        opened
            .recv()
            .map_err(|error| VideoError::Thread(error.to_string()))??;
        Ok((
            Self {
                stop,
                inset,
                keyframe,
                preview,
            },
            received,
        ))
    }

    /// The user's own picture, to show them.
    #[must_use]
    pub fn preview(&self) -> VideoFeed {
        self.preview.clone()
    }

    /// Makes the next picture sent a keyframe: the other end lost some and
    /// cannot go on without one.
    pub fn request_keyframe(&self) {
        self.keyframe.store(true, Ordering::Relaxed);
    }
}

/// The platform's camera input in FFmpeg.
fn input_format() -> &'static str {
    if cfg!(target_os = "macos") {
        "avfoundation"
    } else if cfg!(target_os = "windows") {
        "dshow"
    } else {
        "v4l2"
    }
}

/// The open camera and what turns its pictures into the call's.
struct Capture {
    input: ffmpeg::format::context::Input,
    stream: usize,
    decoder: ffmpeg::decoder::Video,
}

impl Capture {
    fn open((name, path): (&str, String)) -> Result<Self, VideoError> {
        let failed = |error: ffmpeg::Error| VideoError::Camera(error.to_string());
        ffmpeg::init().map_err(failed)?;
        ffmpeg::device::register_all();
        let format = ffmpeg::device::input::video()
            // FFmpeg names some inputs by every alias, comma-separated:
            // V4L2's is "video4linux2,v4l2".
            .find(|format| format.name().split(',').any(|alias| alias == name))
            .ok_or_else(|| VideoError::Camera(format!("this build has no {name} input")))?;
        let mut options = Dictionary::new();
        options.set("framerate", &FRAME_RATE.to_string());
        options.set("video_size", "640x480");
        let input = ffmpeg::format::open_with(&path, &format, options)
            .map_err(failed)?
            .input();
        let stream = input
            .streams()
            .best(Type::Video)
            .ok_or_else(|| VideoError::Camera("the camera gives no pictures".into()))?;
        let index = stream.index();
        let decoder = codec::context::Context::from_parameters(stream.parameters())
            .and_then(|context| context.decoder().video())
            .map_err(failed)?;
        Ok(Self {
            input,
            stream: index,
            decoder,
        })
    }

    fn run(
        mut self,
        units: &async_channel::Sender<Vec<u8>>,
        preview: &FeedWriter,
        inset: &Inset,
        (stop, keyframe): (&AtomicBool, &AtomicBool),
    ) {
        let mut encoder = match H264Encoder::new(Source::Camera) {
            Ok(encoder) => encoder,
            Err(error) => {
                tracing::warn!(%error, "no H.264 encoder: the camera stays off");
                return;
            }
        };
        let mut scalers = Scalers::new(Source::Camera.width(), true);
        let every = Duration::from_secs(1) / FRAME_RATE;
        let mut last: Option<Instant> = None;
        let mut decoded = frame::Video::empty();
        let stream = self.stream;
        for (from, packet) in self.input.packets() {
            if stop.load(Ordering::Relaxed) || units.is_closed() {
                return;
            }
            if from.index() != stream || self.decoder.send_packet(&packet).is_err() {
                continue;
            }
            while self.decoder.receive_frame(&mut decoded).is_ok() {
                // A camera faster than the call sends skips pictures, so
                // the call's clock and the pictures agree.
                if last.is_some_and(|last| last.elapsed() < every.mul_f32(0.9)) {
                    continue;
                }
                last = Some(Instant::now());
                let Ok((sent, shown)) = scalers.convert(&decoded) else {
                    continue;
                };
                preview.show(shown);
                if inset.lending() {
                    inset.lend(sent);
                    // Sent again after the screen: the other end needs a
                    // whole picture then.
                    keyframe.store(true, Ordering::Relaxed);
                    continue;
                }
                match encoder.encode(&sent, keyframe.swap(false, Ordering::Relaxed)) {
                    // Full means the call is behind: this picture is lost.
                    Ok(unit) => drop(units.try_send(unit)),
                    Err(error) => tracing::debug!(%error, "could not encode a picture"),
                }
            }
        }
    }
}

/// The scalers from the camera's format: to the YUV the encoder takes,
/// and to the user's own small, mirrored picture.
pub(crate) struct Scalers {
    sent_width: u32,
    /// Whether the user's own picture shows as in a mirror: a camera's
    /// does, a screen's does not.
    mirror: bool,
    made_for: Option<(Pixel, u32, u32)>,
    sent: Option<scaling::Context>,
    shown: Option<scaling::Context>,
}

impl Scalers {
    /// Scalers sending pictures at most `sent_width` wide.
    pub(crate) fn new(sent_width: u32, mirror: bool) -> Self {
        Self {
            sent_width,
            mirror,
            made_for: None,
            sent: None,
            shown: None,
        }
    }

    /// `picture` as the encoder takes it, and as the user sees it.
    pub(crate) fn convert(
        &mut self,
        picture: &frame::Video,
    ) -> Result<(frame::Video, Picture), ffmpeg::Error> {
        let shape = (picture.format(), picture.width(), picture.height());
        if self.made_for != Some(shape) {
            let (sent_width, sent_height) = fit(shape.1, shape.2, self.sent_width);
            let (shown_width, shown_height) = fit(shape.1, shape.2, PREVIEW_WIDTH);
            self.sent = Some(scaling::Context::get(
                shape.0,
                shape.1,
                shape.2,
                Pixel::YUV420P,
                sent_width & !1,
                sent_height & !1,
                scaling::Flags::BILINEAR,
            )?);
            self.shown = Some(scaling::Context::get(
                shape.0,
                shape.1,
                shape.2,
                Pixel::BGRA,
                shown_width,
                shown_height,
                scaling::Flags::BILINEAR,
            )?);
            self.made_for = Some(shape);
        }
        let (Some(sent_scaler), Some(shown_scaler)) = (&mut self.sent, &mut self.shown) else {
            return Err(ffmpeg::Error::Bug);
        };
        let mut sent = frame::Video::empty();
        sent_scaler.run(picture, &mut sent)?;
        let mut shown = frame::Video::empty();
        shown_scaler.run(picture, &mut shown)?;
        let (width, height) = (shown.width(), shown.height());
        let packed = packed_rows(&shown, width as usize * 4, height as usize);
        let bgra = if self.mirror {
            mirrored(packed, width)
        } else {
            packed
        };
        Ok((
            sent,
            Picture {
                width,
                height,
                bgra,
            },
        ))
    }
}

/// `bgra`, rows `width` pixels wide, flipped left to right: people expect
/// to see themselves as in a mirror.
fn mirrored(mut bgra: Vec<u8>, width: u32) -> Vec<u8> {
    for row in bgra.chunks_mut(width as usize * 4) {
        let pixels = row.as_chunks_mut::<4>().0;
        pixels.reverse();
    }
    bgra
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Camera, mirrored};
    use crate::inset::Inset;

    #[test]
    fn a_camera_sends_keyframe_first_and_shows_the_user_themselves() {
        // FFmpeg's own test picture, where a test machine has no camera.
        let (camera, units) = match Camera::open(
            ("lavfi", "testsrc=size=640x480:rate=15".to_owned()),
            Inset::default(),
        ) {
            Ok(opened) => opened,
            Err(error) => panic!("{error}"),
        };
        let first = units.recv_blocking().unwrap_or_default();
        // An IDR's NAL type, 5, after SPS (7) and PPS (8).
        let types: Vec<u8> = first
            .windows(5)
            .filter(|window| window[..4] == [0, 0, 0, 1])
            .map(|window| window[4] & 0x1f)
            .collect();
        assert!(types.contains(&7) && types.contains(&5), "{types:?}");
        std::thread::sleep(Duration::from_millis(50));
        let shown = camera.preview().take();
        assert_eq!(
            shown.map(|picture| (picture.width, picture.height)),
            Some((320, 240))
        );
    }

    #[test]
    fn the_users_own_picture_is_mirrored() {
        let row = vec![1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3];
        assert_eq!(
            mirrored([row.clone(), row].concat(), 3),
            [3, 3, 3, 3, 2, 2, 2, 2, 1, 1, 1, 1].repeat(2)
        );
    }
}
