//! Sharing the screen in a call: its pictures are scaled, encoded as
//! H.264 for the call and shown to the user, as the camera's are.
//!
//! On Linux the desktop's own dialog asks the user what to share
//! (`portal`). Windows and macOS have none, so natizap lists the screens
//! and windows for its own dialog to ask (`desktop`), and captures the one
//! chosen.

use crate::Picture;
use crate::feed::VideoFeed;
use crate::inset::Inset;
use crate::pipeline::Running;
use crate::remote_video::VideoError;

#[cfg(not(target_os = "linux"))]
mod desktop;
#[cfg(target_os = "linux")]
mod portal;

/// A screen or a window the user may share.
#[derive(Debug, Clone)]
pub struct ShareSource {
    /// What to hand [`ScreenShare::start`] to share it.
    pub id: SourceId,
    /// The screen's or the window's name.
    pub name: String,
    /// The app a window belongs to.
    pub app: Option<String>,
    /// A small picture of it, as it is now.
    pub thumbnail: Option<Picture>,
}

/// Which screen or window to share.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceId {
    kind: SourceKind,
    id: u32,
}

impl SourceId {
    /// Whether it is a screen or a window.
    #[must_use]
    pub fn kind(self) -> SourceKind {
        self.kind
    }
}

/// A whole screen, or one window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceKind {
    /// A whole screen.
    Screen,
    /// One window.
    Window,
}

/// Whether natizap asks the user what to share, from
/// [`share_sources`]; where it does not (Linux), the desktop asks, as
/// [`ScreenShare::start`] runs.
#[must_use]
pub fn natizap_asks() -> bool {
    cfg!(not(target_os = "linux"))
}

/// What the user may share, where natizap asks which: every screen, the
/// main one first, then every window but natizap's own and those
/// minimized. Takes a picture of each: run it off the UI thread.
#[must_use]
pub fn share_sources() -> Vec<ShareSource> {
    #[cfg(target_os = "linux")]
    return Vec::new();
    #[cfg(not(target_os = "linux"))]
    desktop::sources()
}

/// The screen shared in a call, until dropped or until the user stops it
/// from the desktop.
#[derive(Debug)]
pub struct ScreenShare {
    running: Running,
}

impl ScreenShare {
    /// Shares `source`, one of [`share_sources`], with the camera `inset`
    /// lends in its corner; where the desktop asks the user what to share
    /// (Linux), `source` is none, and this blocks until they choose: run
    /// it off the UI thread. The receiver gives the call one Annex-B
    /// access unit per picture, up to fifteen a second; it closes when the
    /// sharing stops.
    ///
    /// # Errors
    ///
    /// Fails when the user declines, or when the source cannot be
    /// captured.
    pub fn start(
        source: Option<SourceId>,
        inset: Inset,
    ) -> Result<(Self, async_channel::Receiver<Vec<u8>>), VideoError> {
        #[cfg(target_os = "linux")]
        let (running, units) = {
            // The desktop's dialog chooses instead.
            let _ = source;
            portal::start(inset)?
        };
        #[cfg(not(target_os = "linux"))]
        let (running, units) = {
            let source =
                source.ok_or_else(|| VideoError::Screen("nothing was chosen to share".into()))?;
            chosen::start(source, inset)?
        };
        Ok((Self { running }, units))
    }

    /// What is being shared, to show the user.
    #[must_use]
    pub fn preview(&self) -> VideoFeed {
        self.running.preview()
    }

    /// Makes the next picture sent a keyframe.
    pub fn request_keyframe(&self) {
        self.running.request_keyframe();
    }
}

/// Capturing the source the user chose in natizap's dialog.
#[cfg(not(target_os = "linux"))]
mod chosen {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use ffmpeg::format::Pixel;
    use ffmpeg_next as ffmpeg;

    use super::{SourceId, desktop};
    use crate::h264::FRAME_RATE;
    use crate::inset::Inset;
    use crate::pipeline::{Role, Running, packed, pipeline};
    use crate::remote_video::VideoError;

    pub(super) fn start(
        source: SourceId,
        inset: Inset,
    ) -> Result<(Running, async_channel::Receiver<Vec<u8>>), VideoError> {
        let (seed, ends) = pipeline(Role::Screen, inset);
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        // A plain thread: each capture blocks.
        std::thread::Builder::new()
            .name("natizap-screen".into())
            .spawn(move || {
                let mut pipeline = seed.build();
                let every = Duration::from_secs(1) / FRAME_RATE;
                let recorded = desktop::record(source, every, &stopping, |image| {
                    let (width, height) = image.dimensions();
                    if let Some(picture) = packed(
                        image.as_raw(),
                        Pixel::RGBA,
                        width,
                        height,
                        width as usize * 4,
                    ) {
                        pipeline.take(&picture);
                    }
                    !pipeline.closed()
                });
                if let Err(error) = recorded {
                    tracing::warn!(%error, "the screen stopped being shared");
                }
            })
            .map_err(|error| VideoError::Thread(error.to_string()))?;
        Ok(ends.running(move || stop.store(true, Ordering::Relaxed)))
    }
}
