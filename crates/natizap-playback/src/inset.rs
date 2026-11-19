//! The camera shown in a corner of the shared screen. A call sends one
//! picture: while the screen is shared with the camera on, the camera
//! lends its pictures to the screen, which draws the newest small in its
//! bottom right corner before encoding, instead of sending them itself.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use ffmpeg::format::Pixel;
use ffmpeg::software::scaling;
use ffmpeg::util::frame;
use ffmpeg_next as ffmpeg;

/// The share of the screen's width the camera takes in its corner.
const INSET_SHARE: u32 = 5;
/// The gap between the camera and the screen's edges, in pixels.
const MARGIN: u32 = 16;

/// Where the camera lends its pictures to the shared screen. Clones share
/// one place.
#[derive(Clone, Default)]
pub struct Inset(Arc<Shared>);

impl std::fmt::Debug for Inset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inset")
            .field("lending", &self.lending())
            .finish_non_exhaustive()
    }
}

#[derive(Default)]
struct Shared {
    lending: AtomicBool,
    /// The camera's newest picture, YUV 4:2:0, drawn into every picture of
    /// the screen until a newer one comes.
    newest: Mutex<Option<frame::Video>>,
}

impl Inset {
    /// Has the camera lend its pictures to the screen (`true`) or send them
    /// itself.
    pub fn set_lending(&self, lending: bool) {
        self.0.lending.store(lending, Ordering::Relaxed);
        if !lending {
            *self.slot() = None;
        }
    }

    pub(crate) fn lending(&self) -> bool {
        self.0.lending.load(Ordering::Relaxed)
    }

    /// The camera's newest picture, while it lends them.
    pub(crate) fn lend(&self, picture: frame::Video) {
        if self.lending() {
            *self.slot() = Some(picture);
        }
    }

    /// The camera stopped: its last picture leaves the screen.
    pub(crate) fn clear(&self) {
        *self.slot() = None;
    }

    fn slot(&self) -> std::sync::MutexGuard<'_, Option<frame::Video>> {
        // A frame is whole or absent whatever a panicking holder did.
        self.0.newest.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Draws the camera into each picture of the screen.
#[derive(Default)]
pub(crate) struct Corner {
    scaler: Option<((u32, u32, u32, u32), scaling::Context)>,
}

impl Corner {
    /// `base`, a YUV 4:2:0 picture of the screen, with the camera's newest
    /// picture from `inset` in its bottom right corner while it lends them.
    pub(crate) fn draw(&mut self, inset: &Inset, base: &mut frame::Video) {
        if !inset.lending() {
            return;
        }
        let scaled = {
            let newest = inset.slot();
            let Some(picture) = newest.as_ref() else {
                return;
            };
            self.scale(picture, base.width())
        };
        if let Some(scaled) = scaled {
            let x = base.width().saturating_sub(scaled.width() + MARGIN) & !1;
            let y = base.height().saturating_sub(scaled.height() + MARGIN) & !1;
            overlay(base, &scaled, x, y);
        }
    }

    /// `picture` scaled to a fifth of a screen `base_width` wide.
    fn scale(&mut self, picture: &frame::Video, base_width: u32) -> Option<frame::Video> {
        let (width, height) = (picture.width(), picture.height());
        if width == 0 || height == 0 {
            return None;
        }
        let inset_width = (base_width / INSET_SHARE).max(2) & !1;
        let inset_height =
            u32::try_from(u64::from(inset_width) * u64::from(height) / u64::from(width))
                .unwrap_or(u32::MAX)
                & !1;
        let shape = (width, height, inset_width, inset_height);
        let scaler = match &mut self.scaler {
            Some((made_for, scaler)) if *made_for == shape => scaler,
            slot => {
                let scaler = scaling::Context::get(
                    picture.format(),
                    width,
                    height,
                    Pixel::YUV420P,
                    inset_width,
                    inset_height,
                    scaling::Flags::BILINEAR,
                )
                .ok()?;
                &mut slot.insert((shape, scaler)).1
            }
        };
        let mut small = frame::Video::empty();
        scaler.run(picture, &mut small).ok()?;
        Some(small)
    }
}

/// Copies `top`, YUV 4:2:0, into `base` with its top left corner at `x`,
/// `y`, both even; whatever falls outside `base` is left out.
fn overlay(base: &mut frame::Video, top: &frame::Video, x: u32, y: u32) {
    for plane in 0..3 {
        let shift = u32::from(plane > 0);
        let (x, y) = ((x >> shift) as usize, (y >> shift) as usize);
        let width = (top.width() >> shift) as usize;
        let height = (top.height() >> shift) as usize;
        let base_width = (base.width() >> shift) as usize;
        let base_height = (base.height() >> shift) as usize;
        let columns = width.min(base_width.saturating_sub(x));
        let rows = height.min(base_height.saturating_sub(y));
        let (from_stride, to_stride) = (top.stride(plane), base.stride(plane));
        let from = top.data(plane).to_vec();
        let to = base.data_mut(plane);
        for row in 0..rows {
            let at = (y + row) * to_stride + x;
            let source = row * from_stride;
            to[at..at + columns].copy_from_slice(&from[source..source + columns]);
        }
    }
}

#[cfg(test)]
mod tests {
    use ffmpeg::format::Pixel;
    use ffmpeg::util::frame;
    use ffmpeg_next as ffmpeg;

    use super::{Corner, Inset, overlay};

    fn picture(width: u32, height: u32, shade: u8) -> frame::Video {
        let mut picture = frame::Video::new(Pixel::YUV420P, width, height);
        for plane in 0..3 {
            picture.data_mut(plane).fill(shade);
        }
        picture
    }

    #[test]
    fn the_camera_lands_in_the_bottom_right_corner() {
        let mut base = picture(64, 32, 0);
        overlay(&mut base, &picture(8, 4, 200), 40, 20);
        let stride = base.stride(0);
        let luma = base.data(0);
        assert_eq!(luma[20 * stride + 40], 200);
        assert_eq!(luma[23 * stride + 47], 200);
        assert_eq!(luma[19 * stride + 40], 0, "above it");
        assert_eq!(luma[20 * stride + 48], 0, "beside it");
        let chroma = base.data(1);
        assert_eq!(chroma[10 * base.stride(1) + 20], 200);
    }

    #[test]
    fn the_screen_shows_the_camera_only_while_it_lends() {
        let inset = Inset::default();
        let mut corner = Corner::default();
        inset.lend(picture(64, 48, 200));
        let mut base = picture(640, 360, 0);
        corner.draw(&inset, &mut base);
        assert!(
            base.data(0).iter().all(|&luma| luma == 0),
            "drew before lending"
        );
        inset.set_lending(true);
        inset.lend(picture(64, 48, 200));
        corner.draw(&inset, &mut base);
        assert!(base.data(0).contains(&200));
        // The camera's last picture stays in the corner between its own.
        let mut next = picture(640, 360, 0);
        corner.draw(&inset, &mut next);
        assert!(next.data(0).contains(&200));
        // Until the camera goes off.
        inset.clear();
        let mut after = picture(640, 360, 0);
        corner.draw(&inset, &mut after);
        assert!(after.data(0).iter().all(|&luma| luma == 0));
    }
}
