//! Sharing the screen on Linux: the desktop portal asks the user what to
//! share, which on Wayland is the only way an app may see the screen, and
//! the pictures come over `PipeWire`, read through libpipewire since
//! FFmpeg has no `PipeWire` input.

use std::os::fd::OwnedFd;

use ashpd::desktop::CreateSessionOptions;
use ashpd::desktop::PersistMode;
use ashpd::desktop::screencast::{
    CursorMode, OpenPipeWireRemoteOptions, Screencast, SelectSourcesOptions, SourceType,
    StartCastOptions,
};
use ffmpeg::format::Pixel;
use ffmpeg_next as ffmpeg;
use pipewire as pw;
use pw::spa;
use pw::spa::param::video::{VideoFormat, VideoInfoRaw};
use pw::spa::pod::Pod;

use crate::h264::FRAME_RATE;
use crate::inset::Inset;
use crate::pipeline::{Pipeline, Role, Running, packed, pipeline};
use crate::remote_video::VideoError;

pub(super) fn start(
    inset: Inset,
) -> Result<(Running, async_channel::Receiver<Vec<u8>>), VideoError> {
    let (fd, node) = futures::executor::block_on(choose())
        .map_err(|error| VideoError::Screen(error.to_string()))?;
    let (seed, ends) = pipeline(Role::Screen, inset);
    let (stop, stopped) = pw::channel::channel();
    // A plain thread: PipeWire's loop blocks while it runs.
    std::thread::Builder::new()
        .name("natizap-screen".into())
        .spawn(move || {
            if let Err(error) = capture(fd, node, seed.build(), stopped) {
                tracing::warn!(%error, "the screen stopped being shared");
            }
        })
        .map_err(|error| VideoError::Thread(error.to_string()))?;
    Ok(ends.running(move || {
        // The thread may have stopped already, when the user stopped
        // sharing from the desktop: nothing is left to tell then.
        let _ = stop.send(());
    }))
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
    pipeline: Pipeline,
}

/// Reads the shared screen's pictures until stopped, or until the
/// desktop stops the sharing.
fn capture(
    fd: OwnedFd,
    node: u32,
    pipeline: Pipeline,
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
        pipeline,
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
        if !self.pipeline.due() {
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
        if let Some(picture) = packed(bytes, pixel, size.width, size.height, stride) {
            self.pipeline.take(&picture);
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
