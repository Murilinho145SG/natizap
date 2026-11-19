//! Sharing the screen in a call, in place of the camera: in a video call,
//! or in a voice call, which becomes a video call for it. Unlike WhatsApp,
//! no camera is needed: the screen is the call's picture by itself.

use gpui::{AppContext as _, Context};
use natizap_playback::ScreenShare;
use natizap_protocol::LiveCall;

use super::lifecycle::logged;
use super::video::CameraState;
use super::{CallDesk, Going};
use crate::screen::Screen;

/// Whether the screen is shared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sharing {
    /// It is not.
    Off,
    /// The desktop asks the user what to share.
    Choosing,
    /// It is, in place of the camera.
    On,
}

impl CallDesk {
    /// Starts sharing the screen, or stops.
    pub fn toggle_screen(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut() else {
            return;
        };
        let Going::Live(live) = &call.going else {
            return;
        };
        let live = live.clone();
        match call.video.as_ref().map(|video| video.sharing) {
            Some(Sharing::On | Sharing::Choosing) => self.stop_sharing(&live, cx),
            _ => self.share(&live, cx),
        }
    }

    /// Makes sure the call sends pictures, then asks the desktop what to
    /// share off the UI thread.
    fn share(&mut self, live: &LiveCall, cx: &mut Context<Self>) {
        let has_video = self.call.as_ref().is_some_and(|call| call.video.is_some());
        if !has_video {
            let Some((video, ends)) = Self::video_for_call(cx) else {
                return;
            };
            let started = self.protocol.read(cx).start_video(live, ends, cx);
            logged(started, "could not turn the call into a video call", cx);
            if let Some(call) = self.call.as_mut() {
                call.video = Some(video);
            }
            self.show_video();
        }
        let Some(call) = self.call.as_mut() else {
            return;
        };
        let serial = call.serial;
        let Some(video) = call.video.as_mut() else {
            return;
        };
        if !video.sending {
            video.sending = true;
            let ends = video.fresh_ends();
            let resumed = self.protocol.read(cx).resume_video(live, ends, cx);
            logged(resumed, "could not send pictures again", cx);
        }
        video.sharing = Sharing::Choosing;
        cx.notify();
        // The desktop's dialog waits for the user: never on the UI thread.
        let choosing = cx.background_spawn(async { ScreenShare::start() });
        let live = live.clone();
        cx.spawn(async move |this, cx| {
            let chosen = choosing.await;
            let _ = this.update(cx, |desk, cx| desk.shared(serial, &live, chosen, cx));
        })
        .detach();
    }

    /// The desktop answered what to share.
    fn shared(
        &mut self,
        serial: u64,
        live: &LiveCall,
        chosen: Result<
            (ScreenShare, async_channel::Receiver<Vec<u8>>),
            natizap_playback::VideoError,
        >,
        cx: &mut Context<Self>,
    ) {
        let Some(video) = self.current(serial).and_then(|call| call.video.as_mut()) else {
            return;
        };
        // Stopped, or the call ended, while the user chose.
        if video.sharing != Sharing::Choosing {
            return;
        }
        let (share, units) = match chosen {
            Ok(chosen) => chosen,
            Err(error) => {
                tracing::warn!(%error, "the screen is not shared");
                return self.stop_sharing(live, cx);
            }
        };
        video.camera_after = matches!(video.camera, CameraState::On | CameraState::Opening);
        video.camera_off();
        let preview = cx.new(|cx| Screen::new(share.preview(), cx));
        cx.observe(&preview, |_, _, cx| cx.notify()).detach();
        video.preview = Some(preview);
        let sent = video.sent.clone();
        let ending = live.clone();
        video.feeding = Some(cx.spawn(async move |this, cx| {
            while let Ok(unit) = units.recv().await {
                if sent.send(unit).await.is_err() {
                    return;
                }
            }
            // The desktop stopped the sharing, from its own indicator.
            let _ = this.update(cx, |desk, cx| desk.stop_sharing(&ending, cx));
        }));
        video.shared = Some(share);
        video.sharing = Sharing::On;
        let told = self.protocol.read(cx).share_screen(live, true, cx);
        logged(
            told,
            "could not tell the other end the screen is shared",
            cx,
        );
        cx.notify();
    }

    /// Stops sharing the screen: the camera comes back if it was on, else
    /// the call stops sending pictures.
    fn stop_sharing(&mut self, live: &LiveCall, cx: &mut Context<Self>) {
        let Some(video) = self.call.as_mut().and_then(|call| call.video.as_mut()) else {
            return;
        };
        let was_on = video.sharing == Sharing::On;
        video.sharing = Sharing::Off;
        video.shared = None;
        video.feeding = None;
        video.preview = None;
        let camera_after = std::mem::take(&mut video.camera_after);
        if was_on {
            let told = self.protocol.read(cx).share_screen(live, false, cx);
            logged(
                told,
                "could not tell the other end the screen is no longer shared",
                cx,
            );
        }
        if camera_after {
            self.open_camera(cx);
        } else if let Some(video) = self.call.as_mut().and_then(|call| call.video.as_mut())
            && video.sending
            && video.camera == CameraState::Off
        {
            video.sending = false;
            let stopped = self.protocol.read(cx).stop_video(live, cx);
            logged(
                stopped,
                "could not tell the other end the pictures stopped",
                cx,
            );
        }
        cx.notify();
    }
}
