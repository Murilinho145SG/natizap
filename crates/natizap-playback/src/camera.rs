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
use crate::feed::{FeedWriter, VideoFeed, feed};
use crate::h264::{FRAME_RATE, H264Encoder};
use crate::remote_video::VideoError;

/// The widest picture sent: what a phone shows a call in, at the bitrate
/// a call can count on.
const SENT_WIDTH: u32 = 640;
/// The widest the user's own picture is drawn: it shows small.
const PREVIEW_WIDTH: u32 = 320;
/// Access units waiting for the call before new ones are dropped.
const BACKLOG: usize = 4;

/// The camera, open for a call until dropped.
#[derive(Debug)]
pub struct Camera {
    stop: Arc<AtomicBool>,
    keyframe: Arc<AtomicBool>,
    preview: VideoFeed,
}

impl Drop for Camera {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Camera {
    /// Opens the default camera. The receiver gives the call one Annex-B
    /// access unit per picture, fifteen a second. Blocks while the
    /// camera opens; run it off the UI thread.
    ///
    /// # Errors
    ///
    /// Fails when there is no camera or it will not open.
    pub fn start() -> Result<(Self, async_channel::Receiver<Vec<u8>>), VideoError> {
        Self::open(device())
    }

    /// Opens the device `input` names: an FFmpeg input format and what
    /// it opens.
    fn open(
        input: (&'static str, &'static str),
    ) -> Result<(Self, async_channel::Receiver<Vec<u8>>), VideoError> {
        let (units, received) = async_channel::bounded(BACKLOG);
        let (writer, preview) = feed();
        let (stop, keyframe) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(true)),
        );
        let (opened_tx, opened) = std::sync::mpsc::channel();
        let (stopping, wanting) = (Arc::clone(&stop), Arc::clone(&keyframe));
        // A plain thread: reading the camera blocks.
        std::thread::Builder::new()
            .name("natizap-camera".into())
            .spawn(move || match Capture::open(input) {
                Ok(capture) => {
                    let _ = opened_tx.send(Ok(()));
                    capture.run(&units, &writer, &stopping, &wanting);
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

/// The platform's camera input and the device it names.
fn device() -> (&'static str, &'static str) {
    if cfg!(target_os = "macos") {
        ("avfoundation", "0")
    } else if cfg!(target_os = "windows") {
        ("dshow", "video=0")
    } else {
        ("v4l2", "/dev/video0")
    }
}

/// The open camera and what turns its pictures into the call's.
struct Capture {
    input: ffmpeg::format::context::Input,
    stream: usize,
    decoder: ffmpeg::decoder::Video,
}

impl Capture {
    fn open((name, path): (&str, &str)) -> Result<Self, VideoError> {
        let failed = |error: ffmpeg::Error| VideoError::Camera(error.to_string());
        ffmpeg::init().map_err(failed)?;
        ffmpeg::device::register_all();
        let format = ffmpeg::device::input::video()
            .find(|format| format.name() == name)
            .ok_or_else(|| VideoError::Camera(format!("this build has no {name} input")))?;
        let mut options = Dictionary::new();
        options.set("framerate", &FRAME_RATE.to_string());
        options.set("video_size", "640x480");
        let input = ffmpeg::format::open_with(path, &format, options)
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
        stop: &AtomicBool,
        keyframe: &AtomicBool,
    ) {
        let mut encoder = match H264Encoder::new() {
            Ok(encoder) => encoder,
            Err(error) => {
                tracing::warn!(%error, "no H.264 encoder: the camera stays off");
                return;
            }
        };
        let mut scalers = Scalers::default();
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
#[derive(Default)]
struct Scalers {
    made_for: Option<(Pixel, u32, u32)>,
    sent: Option<scaling::Context>,
    shown: Option<scaling::Context>,
}

impl Scalers {
    fn convert(
        &mut self,
        picture: &frame::Video,
    ) -> Result<(frame::Video, Picture), ffmpeg::Error> {
        let shape = (picture.format(), picture.width(), picture.height());
        if self.made_for != Some(shape) {
            let (sent_width, sent_height) = fit(shape.1, shape.2, SENT_WIDTH);
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
        let bgra = mirrored(
            packed_rows(&shown, width as usize * 4, height as usize),
            width,
        );
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

    #[test]
    fn a_camera_sends_keyframe_first_and_shows_the_user_themselves() {
        // FFmpeg's own test picture, where a test machine has no camera.
        let (camera, units) = match Camera::open(("lavfi", "testsrc=size=640x480:rate=15")) {
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
