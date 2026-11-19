//! What turns a captured picture into the call's: kept to the call's
//! cadence, scaled to what is sent and to what the user sees, the camera
//! drawn into a shared screen's corner (or lent to it), and encoded as
//! H.264. The camera and every way of sharing the screen feed one.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ffmpeg::format::Pixel;
use ffmpeg::software::scaling;
use ffmpeg::util::frame;
use ffmpeg_next as ffmpeg;

use crate::Picture;
use crate::decode::{fit, packed_rows};
use crate::feed::{FeedWriter, VideoFeed, feed};
use crate::h264::{FRAME_RATE, H264Encoder, Source};
use crate::inset::{Corner, Inset};

/// Access units waiting for the call before new ones are dropped.
const BACKLOG: usize = 4;

/// What a captured picture is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    /// The camera: mirrored for the user, and lent to a shared screen's
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
}

/// A capture running on a thread of its own, until dropped.
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
    /// What is captured, to show the user.
    pub(crate) fn preview(&self) -> VideoFeed {
        self.preview.clone()
    }

    /// Makes the next picture sent a keyframe.
    pub(crate) fn request_keyframe(&self) {
        self.keyframe.store(true, Ordering::Relaxed);
    }
}

/// The ends of a pipeline its capture's owner keeps.
pub(crate) struct Ends {
    keyframe: Arc<AtomicBool>,
    preview: VideoFeed,
    units: async_channel::Receiver<Vec<u8>>,
}

impl Ends {
    /// The capture, which `stop` ends, and the call's access units, one
    /// per picture sent.
    pub(crate) fn running(
        self,
        stop: impl FnMut() + Send + 'static,
    ) -> (Running, async_channel::Receiver<Vec<u8>>) {
        let running = Running {
            keyframe: self.keyframe,
            preview: self.preview,
            stop: Box::new(stop),
        };
        (running, self.units)
    }
}

/// A pipeline for `role`'s pictures, to build on the capture's thread
/// (its scalers stay on the thread that made them), and the ends its
/// owner keeps.
pub(crate) fn pipeline(role: Role, inset: Inset) -> (Seed, Ends) {
    let (units, received) = async_channel::bounded(BACKLOG);
    let (writer, preview) = feed();
    let keyframe = Arc::new(AtomicBool::new(true));
    let seed = Seed {
        role,
        units,
        writer,
        inset,
        keyframe: Arc::clone(&keyframe),
    };
    let ends = Ends {
        keyframe,
        preview,
        units: received,
    };
    (seed, ends)
}

/// What a pipeline is built from, on the capture's thread.
pub(crate) struct Seed {
    role: Role,
    units: async_channel::Sender<Vec<u8>>,
    writer: FeedWriter,
    inset: Inset,
    keyframe: Arc<AtomicBool>,
}

impl Seed {
    /// The pipeline, on the thread that feeds it.
    pub(crate) fn build(self) -> Pipeline {
        let source = self.role.source();
        Pipeline {
            role: self.role,
            encoder: H264Encoder::new(source)
                .inspect_err(|error| tracing::warn!(%error, "no H.264 encoder: nothing is sent"))
                .ok(),
            scalers: Scalers::new(source.width(), self.role == Role::Camera),
            corner: Corner::default(),
            units: self.units,
            writer: self.writer,
            inset: self.inset,
            keyframe: self.keyframe,
            last: None,
        }
    }
}

/// Turns captured pictures into the call's.
pub(crate) struct Pipeline {
    role: Role,
    encoder: Option<H264Encoder>,
    scalers: Scalers,
    corner: Corner,
    units: async_channel::Sender<Vec<u8>>,
    writer: FeedWriter,
    inset: Inset,
    keyframe: Arc<AtomicBool>,
    last: Option<Instant>,
}

impl Pipeline {
    /// Whether a picture now keeps the call's cadence: a source faster
    /// than the call sends skips pictures, so the call's clock and the
    /// pictures agree.
    pub(crate) fn due(&self) -> bool {
        let every = Duration::from_secs(1) / FRAME_RATE;
        self.last
            .is_none_or(|last| last.elapsed() >= every.mul_f32(0.9))
    }

    /// Whether the call stopped taking pictures.
    pub(crate) fn closed(&self) -> bool {
        self.units.is_closed()
    }

    /// Shows `picture` to the user and sends it to the call.
    pub(crate) fn take(&mut self, picture: &frame::Video) {
        self.last = Some(Instant::now());
        let Ok((mut sent, shown)) = self.scalers.convert(picture) else {
            return;
        };
        self.writer.show(shown);
        match self.role {
            Role::Screen => self.corner.draw(&self.inset, &mut sent),
            Role::Camera if self.inset.lending() => {
                self.inset.lend(sent);
                // Sent again after the screen: the other end needs a
                // whole picture then.
                self.keyframe.store(true, Ordering::Relaxed);
                return;
            }
            Role::Camera => {}
        }
        let Some(encoder) = self.encoder.as_mut() else {
            return;
        };
        match encoder.encode(&sent, self.keyframe.swap(false, Ordering::Relaxed)) {
            // Full means the call is behind: this picture is lost.
            Ok(unit) => drop(self.units.try_send(unit)),
            Err(error) => tracing::debug!(%error, "could not encode a picture"),
        }
    }
}

/// The scalers from a source's format: to the YUV the encoder takes, and
/// to what the user sees, as large: the user may focus their own picture.
struct Scalers {
    sent_width: u32,
    /// Whether the user's own picture shows as in a mirror: a camera's
    /// does, a screen's does not.
    mirror: bool,
    made_for: Option<(Pixel, u32, u32)>,
    sent: Option<scaling::Context>,
    shown: Option<scaling::Context>,
}

impl Scalers {
    fn new(sent_width: u32, mirror: bool) -> Self {
        Self {
            sent_width,
            mirror,
            made_for: None,
            sent: None,
            shown: None,
        }
    }

    /// `picture` as the encoder takes it, and as the user sees it.
    fn convert(
        &mut self,
        picture: &frame::Video,
    ) -> Result<(frame::Video, Picture), ffmpeg::Error> {
        let shape = (picture.format(), picture.width(), picture.height());
        if self.made_for != Some(shape) {
            let (width, height) = fit(shape.1, shape.2, self.sent_width);
            self.sent = Some(scaling::Context::get(
                shape.0,
                shape.1,
                shape.2,
                Pixel::YUV420P,
                width & !1,
                height & !1,
                scaling::Flags::BILINEAR,
            )?);
            self.shown = Some(scaling::Context::get(
                shape.0,
                shape.1,
                shape.2,
                Pixel::BGRA,
                width,
                height,
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

/// `bytes`, 4 bytes a pixel in `pixel`'s order and rows `stride` apart, as
/// a frame FFmpeg can scale.
pub(crate) fn packed(
    bytes: &[u8],
    pixel: Pixel,
    width: u32,
    height: u32,
    stride: usize,
) -> Option<frame::Video> {
    let row = width as usize * 4;
    if width == 0 || height == 0 || stride < row || bytes.len() < stride * height as usize {
        return None;
    }
    let mut picture = frame::Video::new(pixel, width, height);
    let into = picture.stride(0);
    let data = picture.data_mut(0);
    for (index, from) in bytes.chunks(stride).take(height as usize).enumerate() {
        data[index * into..index * into + row].copy_from_slice(&from[..row]);
    }
    Some(picture)
}

#[cfg(test)]
mod tests {
    use ffmpeg::format::Pixel;
    use ffmpeg_next as ffmpeg;

    use super::{Role, mirrored, packed, pipeline};
    use crate::inset::Inset;

    #[test]
    fn the_user_sees_themselves_as_in_a_mirror() {
        let row = vec![1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3];
        assert_eq!(mirrored(row, 3), vec![3, 3, 3, 3, 2, 2, 2, 2, 1, 1, 1, 1]);
    }

    #[test]
    fn a_padded_buffer_becomes_a_picture() -> Result<(), &'static str> {
        // Two rows of two pixels, each row padded to 12 bytes.
        let bytes = [
            1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 0, 0, 9, 9, 9, 9, 8, 8, 8, 8, 0, 0, 0, 0,
        ];
        let shown = packed(&bytes, Pixel::BGRZ, 2, 2, 12).ok_or("no picture")?;
        let stride = shown.stride(0);
        assert_eq!(&shown.data(0)[..8], &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(
            &shown.data(0)[stride..stride + 8],
            &[9, 9, 9, 9, 8, 8, 8, 8]
        );
        Ok(())
    }

    #[test]
    fn a_short_buffer_is_no_picture() {
        assert!(packed(&[0; 10], Pixel::BGRZ, 2, 2, 8).is_none());
    }

    #[test]
    fn a_screen_is_sent_as_wide_as_screens_are_and_shown_unmirrored() -> Result<(), &'static str> {
        let (width, height) = (1920, 1080);
        // Left half white, right half black.
        let row: Vec<u8> = (0..width)
            .flat_map(|x| {
                if x < width / 2 {
                    [255; 4]
                } else {
                    [0, 0, 0, 255]
                }
            })
            .collect();
        let bytes = row.repeat(height as usize);
        let screen =
            packed(&bytes, Pixel::RGBA, width, height, width as usize * 4).ok_or("no picture")?;
        let (seed, ends) = pipeline(Role::Screen, Inset::default());
        let mut pipeline = seed.build();
        pipeline.take(&screen);
        let first = ends.units.try_recv().map_err(|_| "nothing was sent")?;
        assert!(
            first
                .windows(5)
                .any(|window| window[..4] == [0, 0, 0, 1] && window[4] & 0x1f == 5),
            "the first picture is a keyframe"
        );
        let shown = ends.preview.take().ok_or("nothing was shown")?;
        assert_eq!((shown.width, shown.height), (1280, 720));
        // Not mirrored: the white half stays on the left.
        assert!(shown.bgra[0] > 200, "{}", shown.bgra[0]);
        Ok(())
    }
}
