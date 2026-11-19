//! A call's camera: captured through FFmpeg's own device inputs (V4L2 on
//! Linux, `AVFoundation` on macOS, `DirectShow` on Windows), and handed to
//! the call's pipeline, which mirrors it for the user.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ffmpeg::media::Type;
use ffmpeg::util::frame;
use ffmpeg::{Dictionary, codec};
use ffmpeg_next as ffmpeg;

use crate::devices::default_camera;
use crate::feed::VideoFeed;
use crate::h264::FRAME_RATE;
use crate::inset::Inset;
use crate::pipeline::{Pipeline, Role, Running, pipeline};
use crate::remote_video::VideoError;

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
        let (seed, ends) = pipeline(Role::Camera, inset.clone());
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let (opened_tx, opened) = std::sync::mpsc::channel();
        // A plain thread: reading the camera blocks.
        std::thread::Builder::new()
            .name("natizap-camera".into())
            .spawn(move || match Capture::open(input) {
                Ok(capture) => {
                    let _ = opened_tx.send(Ok(()));
                    capture.run(seed.build(), &stopping);
                }
                Err(error) => {
                    let _ = opened_tx.send(Err(error));
                }
            })
            .map_err(|error| VideoError::Thread(error.to_string()))?;
        opened
            .recv()
            .map_err(|error| VideoError::Thread(error.to_string()))??;
        let (running, units) = ends.running(move || stop.store(true, Ordering::Relaxed));
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

/// The open camera and the decoder of its pictures.
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

    fn run(mut self, mut pipeline: Pipeline, stop: &AtomicBool) {
        let mut decoded = frame::Video::empty();
        let stream = self.stream;
        for (from, packet) in self.input.packets() {
            if stop.load(Ordering::Relaxed) || pipeline.closed() {
                return;
            }
            if from.index() != stream || self.decoder.send_packet(&packet).is_err() {
                continue;
            }
            while self.decoder.receive_frame(&mut decoded).is_ok() {
                if pipeline.due() {
                    pipeline.take(&decoded);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::Camera;
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
}
