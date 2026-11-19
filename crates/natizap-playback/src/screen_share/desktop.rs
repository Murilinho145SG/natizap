//! The screens and windows to share where the desktop has no dialog of
//! its own to ask the user (Windows and macOS): listed with a small
//! picture of each for natizap's own, and captured, read through xcap.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use image::RgbaImage;
use xcap::{Monitor, Window, XCapResult};

use super::{ShareSource, SourceId, SourceKind};
use crate::Picture;

/// The widest a source's small picture is.
const THUMBNAIL_WIDTH: u32 = 320;
/// The smallest window worth sharing, either way: smaller ones are a
/// desktop's own helpers, not what a user shares.
const LEAST_SIDE: u32 = 64;
/// Captures failing in a row before the sharing ends: the window closed,
/// or the screen went away.
const FAILURES: u32 = 30;

/// Every screen and window to share.
pub(super) fn sources() -> Vec<ShareSource> {
    let mut monitors = Monitor::all().unwrap_or_default();
    monitors.sort_by_key(|monitor| !monitor.is_primary().unwrap_or(false));
    let screens = monitors.iter().enumerate().filter_map(|(index, monitor)| {
        let id = monitor.id().ok()?;
        let name = monitor
            .friendly_name()
            .or_else(|_| monitor.name())
            .ok()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| format!("Screen {}", index + 1));
        Some(ShareSource {
            id: SourceId {
                kind: SourceKind::Screen,
                id,
            },
            name,
            app: None,
            thumbnail: monitor.capture_image().ok().map(|image| thumbnail(&image)),
        })
    });
    let own = std::process::id();
    let windows = Window::all().unwrap_or_default();
    let windows = windows.iter().filter_map(|window| {
        let shown = !window.is_minimized().unwrap_or(true)
            && window.pid().is_ok_and(|pid| pid != own)
            && window.width().unwrap_or(0) >= LEAST_SIDE
            && window.height().unwrap_or(0) >= LEAST_SIDE;
        if !shown {
            return None;
        }
        let title = window
            .title()
            .ok()
            .filter(|title| !title.trim().is_empty())?;
        Some(ShareSource {
            id: SourceId {
                kind: SourceKind::Window,
                id: window.id().ok()?,
            },
            name: title,
            app: window.app_name().ok().filter(|app| !app.trim().is_empty()),
            thumbnail: window.capture_image().ok().map(|image| thumbnail(&image)),
        })
    });
    screens.chain(windows).collect()
}

/// `image` small, as natizap draws it.
fn thumbnail(image: &RgbaImage) -> Picture {
    let width = image.width().clamp(1, THUMBNAIL_WIDTH);
    let height = (u64::from(image.height()) * u64::from(width) / u64::from(image.width().max(1)))
        .clamp(1, u64::from(u32::MAX));
    let height = u32::try_from(height).unwrap_or(u32::MAX);
    let small = image::imageops::thumbnail(image, width, height);
    let mut bgra = small.into_raw();
    for pixel in bgra.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    Picture {
        width,
        height,
        bgra,
    }
}

/// Captures `source` a picture every `every`, handing each to `each`
/// until it says to stop, `stop` is set, or the source cannot be captured
/// any more.
///
/// # Errors
///
/// Fails when the source is gone.
pub(super) fn record(
    source: SourceId,
    every: Duration,
    stop: &AtomicBool,
    each: impl FnMut(&RgbaImage) -> bool,
) -> Result<(), String> {
    let gone = || format!("{source:?} is gone");
    match source.kind {
        SourceKind::Screen => {
            let monitor = Monitor::all()
                .map_err(|error| error.to_string())?
                .into_iter()
                .find(|monitor| monitor.id().is_ok_and(|id| id == source.id))
                .ok_or_else(gone)?;
            repeat(|| monitor.capture_image(), every, stop, each)
        }
        SourceKind::Window => {
            let window = Window::all()
                .map_err(|error| error.to_string())?
                .into_iter()
                .find(|window| window.id().is_ok_and(|id| id == source.id))
                .ok_or_else(gone)?;
            repeat(|| window.capture_image(), every, stop, each)
        }
    }
}

/// Runs `grab` every `every`. xcap takes single pictures, not a stream,
/// so the capture keeps the call's cadence itself.
fn repeat(
    grab: impl Fn() -> XCapResult<RgbaImage>,
    every: Duration,
    stop: &AtomicBool,
    mut each: impl FnMut(&RgbaImage) -> bool,
) -> Result<(), String> {
    let mut failures = 0;
    while !stop.load(Ordering::Relaxed) {
        let started = Instant::now();
        match grab() {
            Ok(image) => {
                failures = 0;
                if !each(&image) {
                    return Ok(());
                }
            }
            Err(error) => {
                failures += 1;
                if failures >= FAILURES {
                    return Err(error.to_string());
                }
            }
        }
        if let Some(rest) = every.checked_sub(started.elapsed()) {
            std::thread::sleep(rest);
        }
    }
    Ok(())
}
