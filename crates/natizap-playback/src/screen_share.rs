//! Sharing the screen in a call: the desktop asks the user what to share,
//! and its pictures are scaled, encoded as H.264 for the call and shown
//! small to the user, as the camera's are.
//!
//! On Linux the desktop portal asks, which on Wayland is the only way an
//! app may see the screen, and the pictures come over `PipeWire`. Other
//! systems have no way here yet.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::feed::VideoFeed;
use crate::remote_video::VideoError;

/// The screen shared in a call, until dropped or until the user stops it
/// from the desktop.
pub struct ScreenShare {
    keyframe: Arc<AtomicBool>,
    preview: VideoFeed,
    #[cfg(target_os = "linux")]
    stop: pipewire::channel::Sender<()>,
}

impl std::fmt::Debug for ScreenShare {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScreenShare").finish_non_exhaustive()
    }
}

impl ScreenShare {
    /// Asks the desktop what to share, and shares it. The receiver gives
    /// the call one Annex-B access unit per picture, up to fifteen a
    /// second; it closes when the sharing stops. Blocks while the user
    /// chooses: run it off the UI thread.
    ///
    /// # Errors
    ///
    /// Fails when the user declines, or when the screen cannot be shared
    /// on this system.
    pub fn start() -> Result<(Self, async_channel::Receiver<Vec<u8>>), VideoError> {
        #[cfg(target_os = "linux")]
        return linux::start();
        #[cfg(not(target_os = "linux"))]
        Err(VideoError::Screen(
            "sharing the screen is not available on this system yet".into(),
        ))
    }

    /// What is being shared, to show the user.
    #[must_use]
    pub fn preview(&self) -> VideoFeed {
        self.preview.clone()
    }

    /// Makes the next picture sent a keyframe.
    pub fn request_keyframe(&self) {
        self.keyframe
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(target_os = "linux")]
impl Drop for ScreenShare {
    fn drop(&mut self) {
        // The thread may have stopped already, when the user stopped
        // sharing from the desktop: nothing is left to tell then.
        let _ = self.stop.send(());
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::os::fd::OwnedFd;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use ashpd::desktop::CreateSessionOptions;
    use ashpd::desktop::PersistMode;
    use ashpd::desktop::screencast::{
        CursorMode, OpenPipeWireRemoteOptions, Screencast, SelectSourcesOptions, SourceType,
        StartCastOptions,
    };
    use ffmpeg::format::Pixel;
    use ffmpeg::util::frame;
    use ffmpeg_next as ffmpeg;
    use pipewire as pw;
    use pw::spa;
    use pw::spa::param::video::{VideoFormat, VideoInfoRaw};
    use pw::spa::pod::Pod;

    use super::ScreenShare;
    use crate::camera::Scalers;
    use crate::feed::{FeedWriter, feed};
    use crate::h264::{FRAME_RATE, H264Encoder, Source};
    use crate::remote_video::VideoError;

    /// Access units waiting for the call before new ones are dropped.
    const BACKLOG: usize = 4;

    pub(super) fn start() -> Result<(ScreenShare, async_channel::Receiver<Vec<u8>>), VideoError> {
        let (fd, node) = futures::executor::block_on(choose())
            .map_err(|error| VideoError::Screen(error.to_string()))?;
        let (units, received) = async_channel::bounded(BACKLOG);
        let (writer, preview) = feed();
        let keyframe = Arc::new(AtomicBool::new(true));
        let (stop, stopped) = pw::channel::channel();
        let wanting = Arc::clone(&keyframe);
        // A plain thread: PipeWire's loop blocks while it runs.
        std::thread::Builder::new()
            .name("natizap-screen".into())
            .spawn(move || {
                if let Err(error) = capture(fd, node, units, writer, &wanting, stopped) {
                    tracing::warn!(%error, "the screen stopped being shared");
                }
            })
            .map_err(|error| VideoError::Thread(error.to_string()))?;
        Ok((
            ScreenShare {
                keyframe,
                preview,
                stop,
            },
            received,
        ))
    }

    /// Asks the desktop what to share: the `PipeWire` connection to it and
    /// the node the pictures come from.
    async fn choose() -> ashpd::Result<(OwnedFd, u32)> {
        let portal = Screencast::new().await?;
        let session = portal
            .create_session(CreateSessionOptions::default())
            .await?;
        portal
            .select_sources(
                &session,
                SelectSourcesOptions::default()
                    .set_cursor_mode(CursorMode::EMBEDDED)
                    .set_sources(SourceType::MONITOR | SourceType::WINDOW)
                    .set_multiple(false)
                    .set_persist_mode(PersistMode::DoNot),
            )
            .await?
            .response()?;
        let streams = portal
            .start(&session, None, StartCastOptions::default())
            .await?
            .response()?;
        let node = streams
            .streams()
            .first()
            .map(ashpd::desktop::screencast::Stream::pipe_wire_node_id)
            .ok_or(ashpd::Error::NoResponse)?;
        let fd = portal
            .open_pipe_wire_remote(&session, OpenPipeWireRemoteOptions::default())
            .await?;
        Ok((fd, node))
    }

    /// What the stream's callbacks share.
    struct Pictures {
        format: VideoInfoRaw,
        encoder: Option<H264Encoder>,
        scalers: Scalers,
        units: async_channel::Sender<Vec<u8>>,
        writer: FeedWriter,
        keyframe: Arc<AtomicBool>,
        last: Option<Instant>,
    }

    /// Reads the shared screen's pictures until stopped, or until the
    /// desktop stops the sharing.
    fn capture(
        fd: OwnedFd,
        node: u32,
        units: async_channel::Sender<Vec<u8>>,
        writer: FeedWriter,
        keyframe: &Arc<AtomicBool>,
        stopped: pw::channel::Receiver<()>,
    ) -> Result<(), pw::Error> {
        pw::init();
        let main = pw::main_loop::MainLoopRc::new(None)?;
        let context = pw::context::ContextRc::new(&main, None)?;
        let core = context.connect_fd_rc(fd, None)?;
        let quitting = main.clone();
        let _stopper = stopped.attach(main.loop_(), move |()| quitting.quit());
        let stream = pw::stream::StreamBox::new(
            &core,
            "natizap-screen",
            pw::properties::properties! {
                *pw::keys::MEDIA_TYPE => "Video",
                *pw::keys::MEDIA_CATEGORY => "Capture",
                *pw::keys::MEDIA_ROLE => "Screen",
            },
        )?;
        let pictures = Pictures {
            format: VideoInfoRaw::default(),
            encoder: H264Encoder::new(Source::Screen)
                .inspect_err(|error| tracing::warn!(%error, "no H.264 encoder for the screen"))
                .ok(),
            scalers: Scalers::new(Source::Screen.width(), false),
            units,
            writer,
            keyframe: Arc::clone(keyframe),
            last: None,
        };
        let ending = main.clone();
        let _listener = stream
            .add_local_listener_with_user_data(pictures)
            .state_changed(move |_, _, _, now| {
                if matches!(
                    now,
                    pw::stream::StreamState::Error(_) | pw::stream::StreamState::Unconnected
                ) {
                    ending.quit();
                }
            })
            .param_changed(|_, pictures, id, param| {
                if let Some(param) = param
                    && id == spa::param::ParamType::Format.as_raw()
                    && pictures.format.parse(param).is_err()
                {
                    tracing::warn!("the screen's picture format could not be read");
                }
            })
            .process(|stream, pictures| {
                if let Some(mut buffer) = stream.dequeue_buffer() {
                    pictures.take(&mut buffer);
                }
            })
            .register()?;
        let formats = formats();
        let Some(format) = Pod::from_bytes(&formats) else {
            return Err(pw::Error::CreationFailed);
        };
        stream.connect(
            spa::utils::Direction::Input,
            Some(node),
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut [format],
        )?;
        main.run();
        Ok(())
    }

    impl Pictures {
        /// Sends the picture in `buffer`, unless it comes sooner than the
        /// call's cadence.
        fn take(&mut self, buffer: &mut pw::buffer::Buffer<'_>) {
            let every = Duration::from_secs(1) / FRAME_RATE;
            if self
                .last
                .is_some_and(|last| last.elapsed() < every.mul_f32(0.9))
            {
                return;
            }
            let Some(pixel) = pixel(self.format.format()) else {
                return;
            };
            let size = self.format.size();
            let Some(data) = buffer.datas_mut().first_mut() else {
                return;
            };
            let stride = usize::try_from(data.chunk().stride()).unwrap_or(0);
            let Some(bytes) = data.data() else {
                return;
            };
            let Some(picture) = picture(bytes, pixel, size.width, size.height, stride) else {
                return;
            };
            self.last = Some(Instant::now());
            let Ok((sent, shown)) = self.scalers.convert(&picture) else {
                return;
            };
            self.writer.show(shown);
            let Some(encoder) = self.encoder.as_mut() else {
                return;
            };
            match encoder.encode(&sent, self.keyframe.swap(false, Ordering::Relaxed)) {
                // Full means the call is behind: this picture is lost.
                Ok(unit) => drop(self.units.try_send(unit)),
                Err(error) => tracing::debug!(%error, "could not encode the screen"),
            }
        }
    }

    /// FFmpeg's name for the packed formats the screen is asked in.
    fn pixel(format: VideoFormat) -> Option<Pixel> {
        match format {
            VideoFormat::BGRx => Some(Pixel::BGRZ),
            VideoFormat::BGRA => Some(Pixel::BGRA),
            VideoFormat::RGBx => Some(Pixel::RGBZ),
            VideoFormat::RGBA => Some(Pixel::RGBA),
            _ => None,
        }
    }

    /// `bytes`, rows `stride` apart, as a frame FFmpeg can scale.
    fn picture(
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

    /// The formats asked of the screen: packed 8-bit colour at any size.
    fn formats() -> Vec<u8> {
        use spa::param::format::{FormatProperties, MediaSubtype, MediaType};
        let object = spa::pod::object!(
            spa::utils::SpaTypes::ObjectParamFormat,
            spa::param::ParamType::EnumFormat,
            spa::pod::property!(FormatProperties::MediaType, Id, MediaType::Video),
            spa::pod::property!(FormatProperties::MediaSubtype, Id, MediaSubtype::Raw),
            spa::pod::property!(
                FormatProperties::VideoFormat,
                Choice,
                Enum,
                Id,
                VideoFormat::BGRx,
                VideoFormat::BGRx,
                VideoFormat::BGRA,
                VideoFormat::RGBx,
                VideoFormat::RGBA
            ),
            spa::pod::property!(
                FormatProperties::VideoSize,
                Choice,
                Range,
                Rectangle,
                spa::utils::Rectangle {
                    width: 1280,
                    height: 720
                },
                spa::utils::Rectangle {
                    width: 1,
                    height: 1
                },
                spa::utils::Rectangle {
                    width: 8192,
                    height: 8192
                }
            ),
            spa::pod::property!(
                FormatProperties::VideoFramerate,
                Choice,
                Range,
                Fraction,
                spa::utils::Fraction {
                    num: FRAME_RATE,
                    denom: 1
                },
                spa::utils::Fraction { num: 0, denom: 1 },
                spa::utils::Fraction { num: 60, denom: 1 }
            ),
        );
        spa::pod::serialize::PodSerializer::serialize(
            std::io::Cursor::new(Vec::new()),
            &spa::pod::Value::Object(object),
        )
        .map_or_else(|_| Vec::new(), |(cursor, _)| cursor.into_inner())
    }

    #[cfg(test)]
    mod tests {
        use ffmpeg::format::Pixel;
        use ffmpeg_next as ffmpeg;

        use super::picture;

        #[test]
        fn a_padded_screen_buffer_becomes_a_picture() -> Result<(), &'static str> {
            // Two rows of two pixels, each row padded to 12 bytes.
            let bytes = [
                1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 0, 0, 9, 9, 9, 9, 8, 8, 8, 8, 0, 0, 0, 0,
            ];
            let shown = picture(&bytes, Pixel::BGRZ, 2, 2, 12).ok_or("no picture")?;
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
            assert!(picture(&[0; 10], Pixel::BGRZ, 2, 2, 8).is_none());
        }
    }
}
