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
use crate::inset::{Corner, Inset};
use crate::remote_video::VideoError;

/// Access units waiting for the call before new ones are dropped.
const BACKLOG: usize = 4;

/// What a captured device's pictures are for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    /// A camera: mirrored for the user, and lent to a shared screen's
    /// corner while one is shared.
    Camera,
    /// A shared screen, with the camera in its corner.
    Screen,
}

impl Role {
    fn source(self) -> Source {
        match self {
            Self::Camera => Source::Camera,
            Self::Screen => Source::Screen,
        }
    }

    fn failed(self, error: String) -> VideoError {
        match self {
            Self::Camera => VideoError::Camera(error),
            Self::Screen => VideoError::Screen(error),
        }
    }
}

/// A device captured on a thread of its own, until dropped.
pub(crate) struct Running {
    keyframe: Arc<AtomicBool>,
    preview: VideoFeed,
    stop: Box<dyn FnMut() + Send>,
}

impl std::fmt::Debug for Running {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Running").finish_non_exhaustive()
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        (self.stop)();
    }
}

impl Running {
    /// A capture that `stop` ends, asked for keyframes through `keyframe`
    /// and shown through `preview`.
    pub(crate) fn new(
        keyframe: Arc<AtomicBool>,
        preview: VideoFeed,
        stop: impl FnMut() + Send + 'static,
    ) -> Self {
        Self {
            keyframe,
            preview,
            stop: Box::new(stop),
        }
    }

    /// What is captured, to show the user.
    pub(crate) fn preview(&self) -> VideoFeed {
        self.preview.clone()
    }

    /// Makes the next picture sent a keyframe.
    pub(crate) fn request_keyframe(&self) {
        self.keyframe.store(true, Ordering::Relaxed);
    }
}

/// Captures the FFmpeg device `input` names (an input format and what it
/// opens), with `options`, for `role`. The receiver gives the call one
/// Annex-B access unit per picture. Blocks while the device opens.
pub(crate) fn capture(
    input: (&'static str, String),
    options: Vec<(&'static str, String)>,
    role: Role,
    inset: Inset,
) -> Result<(Running, async_channel::Receiver<Vec<u8>>), VideoError> {
    let (units, received) = async_channel::bounded(BACKLOG);
    let (writer, preview) = feed();
    let (stop, keyframe) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(true)),
    );
    let (opened_tx, opened) = std::sync::mpsc::channel();
    let (stopping, wanting) = (Arc::clone(&stop), Arc::clone(&keyframe));
    // A plain thread: reading a device blocks.
    std::thread::Builder::new()
        .name("natizap-capture".into())
        .spawn(move || match Capture::open(input, &options, role) {
            Ok(capture) => {
                let _ = opened_tx.send(Ok(()));
                capture.run(&units, &writer, &inset, (&stopping, &wanting));
            }
            Err(error) => {
                let _ = opened_tx.send(Err(error));
            }
        })
        .map_err(|error| VideoError::Thread(error.to_string()))?;
    opened
        .recv()
        .map_err(|error| VideoError::Thread(error.to_string()))??;
    let running = Running::new(keyframe, preview, move || {
        stop.store(true, Ordering::Relaxed);
    });
    Ok((running, received))
}

/// The camera, open for a call until dropped.
#[derive(Debug)]
pub struct Camera {
    running: Running,
    inset: Inset,
}

impl Drop for Camera {
    fn drop(&mut self) {
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
        let options = vec![
            ("framerate", FRAME_RATE.to_string()),
            ("video_size", "640x480".to_owned()),
        ];
        let (running, units) = capture(input, options, Role::Camera, inset.clone())?;
        Ok((Self { running, inset }, units))
    }

    /// The user's own picture, to show them.
    #[must_use]
    pub fn preview(&self) -> VideoFeed {
        self.running.preview()
    }

    /// Makes the next picture sent a keyframe: the other end lost some and
    /// cannot go on without one.
    pub fn request_keyframe(&self) {
        self.running.request_keyframe();
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

/// The open device and what turns its pictures into the call's.
struct Capture {
    input: ffmpeg::format::context::Input,
    stream: usize,
    decoder: ffmpeg::decoder::Video,
    role: Role,
}

impl Capture {
    fn open(
        (name, path): (&str, String),
        options: &[(&'static str, String)],
        role: Role,
    ) -> Result<Self, VideoError> {
        let failed = |error: ffmpeg::Error| role.failed(error.to_string());
        ffmpeg::init().map_err(failed)?;
        ffmpeg::device::register_all();
        let format = ffmpeg::device::input::video()
            // FFmpeg names some inputs by every alias, comma-separated:
            // V4L2's is "video4linux2,v4l2".
            .find(|format| format.name().split(',').any(|alias| alias == name))
            .ok_or_else(|| role.failed(format!("this build has no {name} input")))?;
        let mut dictionary = Dictionary::new();
        for (key, value) in options {
            dictionary.set(key, value);
        }
        let input = ffmpeg::format::open_with(&path, &format, dictionary)
            .map_err(failed)?
            .input();
        let stream = input
            .streams()
            .best(Type::Video)
            .ok_or_else(|| role.failed("the device gives no pictures".into()))?;
        let index = stream.index();
        let decoder = codec::context::Context::from_parameters(stream.parameters())
            .and_then(|context| context.decoder().video())
            .map_err(failed)?;
        Ok(Self {
            input,
            stream: index,
            decoder,
            role,
        })
    }

    fn run(
        mut self,
        units: &async_channel::Sender<Vec<u8>>,
        preview: &FeedWriter,
        inset: &Inset,
        (stop, keyframe): (&AtomicBool, &AtomicBool),
    ) {
        let source = self.role.source();
        let mut encoder = match H264Encoder::new(source) {
            Ok(encoder) => encoder,
            Err(error) => {
                tracing::warn!(%error, "no H.264 encoder: nothing is sent");
                return;
            }
        };
        let mut scalers = Scalers::new(source.width(), self.role == Role::Camera);
        let mut corner = Corner::default();
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
                // A device faster than the call sends skips pictures, so
                // the call's clock and the pictures agree.
                if last.is_some_and(|last| last.elapsed() < every.mul_f32(0.9)) {
                    continue;
                }
                last = Some(Instant::now());
                let Ok((mut sent, shown)) = scalers.convert(&decoded) else {
                    continue;
                };
                preview.show(shown);
                if self.role == Role::Screen {
                    corner.draw(inset, &mut sent);
                } else if inset.lending() {
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
    /// Scalers sending pictures at most `sent_width` wide, and showing
    /// them to the user as large: the user may focus their own picture.
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
            let (shown_width, shown_height) = fit(shape.1, shape.2, self.sent_width);
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

    use super::{Camera, Role, capture, mirrored};
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
            Some((640, 480))
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

    #[test]
    fn a_screen_is_sent_as_the_user_sees_it() {
        let (screen, units) = match capture(
            ("lavfi", "testsrc=size=1920x1080:rate=15".to_owned()),
            Vec::new(),
            Role::Screen,
            Inset::default(),
        ) {
            Ok(opened) => opened,
            Err(error) => panic!("{error}"),
        };
        let first = units.recv_blocking().unwrap_or_default();
        assert!(
            first
                .windows(5)
                .any(|window| window[..4] == [0, 0, 0, 1] && window[4] & 0x1f == 5),
            "the first picture is a keyframe"
        );
        std::thread::sleep(Duration::from_millis(50));
        let shown = screen.preview().take();
        // As wide as a screen is sent, its shape kept.
        assert_eq!(
            shown.map(|picture| (picture.width, picture.height)),
            Some((1280, 720))
        );
    }
}
