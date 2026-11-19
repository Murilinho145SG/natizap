//! A call's video on this side: the other end's pictures, the camera while
//! it is on, and turning a voice call into a video call either way.

use gpui::{AppContext as _, Context, Entity, Task};
use natizap_playback::{AccessUnits, Camera, ScreenShare, VideoError, decode_video};
use natizap_protocol::Video;

use super::lifecycle::logged;
use super::sharing::Sharing;
use super::{CallDesk, Going};
use crate::screen::Screen;

/// Pictures the camera made waiting for the call before new ones are
/// dropped.
const SENT_BACKLOG: usize = 4;

/// Whether the camera shows the user to the other end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraState {
    /// Off: the other end is told so.
    Off,
    /// Being opened.
    Opening,
    /// On.
    On,
    /// There is no camera, or it would not open.
    Missing,
}

/// A call's video, from the moment the call has it.
pub(super) struct CallVideo {
    pub(super) remote: Entity<Screen>,
    pub(super) preview: Option<Entity<Screen>>,
    pub(super) camera: CameraState,
    /// The open camera, while it is on.
    open: Option<Camera>,
    /// The call's own end of the pictures sent, fed by the camera or the
    /// shared screen.
    pub(super) sent: async_channel::Sender<Vec<u8>>,
    pub(super) feeding: Option<Task<()>>,
    /// Whether the screen is shared, in place of the camera.
    pub(super) sharing: Sharing,
    /// The screen shared, while it is.
    pub(super) shared: Option<ScreenShare>,
    /// The camera was on when sharing began: it comes back after.
    pub(super) camera_after: bool,
    /// Where the other end's pictures go, to hand the call again when the
    /// camera comes back on.
    screen: AccessUnits,
    /// Whether the call sends the other end pictures: off once the camera
    /// was turned off, until it or the screen comes back on.
    pub(super) sending: bool,
    /// Whether the other end's camera is on, as they last said.
    pub(super) peer_on: bool,
}

impl CallVideo {
    /// A call's video, with the ends the call takes.
    fn new(cx: &mut Context<CallDesk>) -> Result<(Self, Video), VideoError> {
        let (feed, screen) = decode_video()?;
        let remote = cx.new(|cx| Screen::new(feed, cx));
        cx.observe(&remote, |_, _, cx| cx.notify()).detach();
        let (sent, camera) = async_channel::bounded(SENT_BACKLOG);
        Ok((
            Self {
                remote,
                preview: None,
                camera: CameraState::Off,
                open: None,
                sent,
                feeding: None,
                sharing: Sharing::Off,
                shared: None,
                camera_after: false,
                screen: screen.clone(),
                sending: true,
                peer_on: true,
            },
            Video { camera, screen },
        ))
    }

    /// Asks the other end's next picture from the camera to be a keyframe.
    pub(super) fn keyframe_wanted(&self) {
        if let Some(camera) = &self.open {
            camera.request_keyframe();
        }
        if let Some(shared) = &self.shared {
            shared.request_keyframe();
        }
    }

    /// Fresh ends for the call to send the camera through again.
    pub(super) fn fresh_ends(&mut self) -> Video {
        let (sent, camera) = async_channel::bounded(SENT_BACKLOG);
        self.sent = sent;
        Video {
            camera,
            screen: self.screen.clone(),
        }
    }

    pub(super) fn camera_off(&mut self) {
        self.open = None;
        self.feeding = None;
        self.preview = None;
        self.camera = CameraState::Off;
    }
}

impl CallDesk {
    /// The video a new call takes: made, with its camera opening.
    pub(super) fn video_for_call(cx: &mut Context<Self>) -> Option<(CallVideo, Video)> {
        match CallVideo::new(cx) {
            Ok(made) => Some(made),
            Err(error) => {
                tracing::warn!(%error, "the call goes on without video");
                None
            }
        }
    }

    /// Turns the camera on or off. A voice call becomes a video call,
    /// asking the other end to turn theirs on too.
    pub fn toggle_camera(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut() else {
            return;
        };
        let Going::Live(live) = &call.going else {
            return;
        };
        let live = live.clone();
        match call.video.as_mut() {
            Some(video) if matches!(video.camera, CameraState::On | CameraState::Opening) => {
                video.camera_off();
                video.sending = false;
                let stopped = self.protocol.read(cx).stop_video(&live, cx);
                logged(
                    stopped,
                    "could not tell the other end the camera went off",
                    cx,
                );
                cx.notify();
            }
            Some(video) => {
                if !video.sending {
                    video.sending = true;
                    let ends = video.fresh_ends();
                    let resumed = self.protocol.read(cx).resume_video(&live, ends, cx);
                    logged(resumed, "could not send the camera again", cx);
                }
                self.open_camera(cx);
            }
            None => {
                let Some((video, ends)) = Self::video_for_call(cx) else {
                    return;
                };
                let started = self.protocol.read(cx).start_video(&live, ends, cx);
                logged(started, "could not turn the call into a video call", cx);
                if let Some(call) = self.call.as_mut() {
                    call.video = Some(video);
                    self.show_video();
                }
                self.open_camera(cx);
            }
        }
    }

    /// Takes up the other end's request to turn the call into a video
    /// call, with the camera on.
    pub fn accept_video(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut() else {
            return;
        };
        let (Going::Live(live), Some(request)) = (&call.going, call.video_request.take()) else {
            return;
        };
        let live = live.clone();
        if call.video.is_none() {
            let Some((video, ends)) = Self::video_for_call(cx) else {
                return;
            };
            let accepted = self
                .protocol
                .read(cx)
                .accept_video(&live, request, ends, cx);
            logged(accepted, "could not take up the request for video", cx);
            if let Some(call) = self.call.as_mut() {
                call.video = Some(video);
                self.show_video();
            }
        }
        self.open_camera(cx);
    }

    /// Leaves the other end's request for video unanswered: the call stays
    /// a voice call.
    pub fn ignore_video(&mut self, cx: &mut Context<Self>) {
        if let Some(call) = self.call.as_mut() {
            call.video_request = None;
            cx.notify();
        }
    }

    /// Moves a camera that is on, or opening, to the one chosen now.
    pub(super) fn reopen_camera(&mut self, cx: &mut Context<Self>) {
        let Some(video) = self.call.as_mut().and_then(|call| call.video.as_mut()) else {
            return;
        };
        if matches!(video.camera, CameraState::On | CameraState::Opening) {
            video.camera_off();
            self.open_camera(cx);
        }
    }

    /// Opens the camera off the UI thread, and feeds the call from it.
    pub(super) fn open_camera(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut() else {
            return;
        };
        let serial = call.serial;
        let Some(video) = call.video.as_mut() else {
            return;
        };
        video.camera = CameraState::Opening;
        cx.notify();
        let device = self.choices.devices.camera.clone();
        let opening = cx.background_spawn(async move { Camera::start(device.as_deref()) });
        cx.spawn(async move |this, cx| {
            let opened = opening.await;
            let _ = this.update(cx, |desk, cx| {
                let Some(video) = desk.current(serial).and_then(|call| call.video.as_mut()) else {
                    return;
                };
                // Turned off again while it opened.
                if video.camera != CameraState::Opening {
                    return;
                }
                match opened {
                    Ok((camera, units)) => {
                        let preview = cx.new(|cx| Screen::new(camera.preview(), cx));
                        cx.observe(&preview, |_, _, cx| cx.notify()).detach();
                        let sent = video.sent.clone();
                        video.feeding = Some(cx.background_spawn(async move {
                            while let Ok(unit) = units.recv().await {
                                if sent.send(unit).await.is_err() {
                                    return;
                                }
                            }
                        }));
                        video.open = Some(camera);
                        video.preview = Some(preview);
                        video.camera = CameraState::On;
                    }
                    Err(error) => {
                        tracing::warn!(%error, "no camera for the call");
                        video.camera = CameraState::Missing;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}
