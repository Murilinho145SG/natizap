//! Sharing the screen in a call: in a video call, or in a voice call,
//! which becomes a video call for it. Unlike WhatsApp, no camera is
//! needed; with one on, it shows in the shared screen's corner, since a
//! call carries one picture.

use std::sync::Arc;

use gpui::{AppContext as _, Context, RenderImage, SharedString};
use natizap_playback::{ScreenShare, ShareSource, SourceId, natizap_asks, share_sources};
use natizap_protocol::LiveCall;

use super::lifecycle::logged;
use super::video::CameraState;
use super::{CallDesk, Going};
use crate::screen::{Screen, render_image};

/// Whether the screen is shared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sharing {
    /// It is not.
    Off,
    /// The user chooses what to share: in natizap's dialog, or in the
    /// desktop's.
    Choosing,
    /// It is, with the camera in its corner while it is on.
    On,
}

/// What natizap's dialog offers to share, while it asks.
#[derive(Debug, Clone)]
pub enum ShareChoices {
    /// The screens and windows are being looked for.
    Finding,
    /// They were found.
    Found(Arc<[ShareChoice]>),
}

/// A screen or window to share, as natizap's dialog shows it.
#[derive(Debug, Clone)]
pub struct ShareChoice {
    /// What to hand [`CallDesk::share_from`].
    pub source: SourceId,
    /// Its name.
    pub name: SharedString,
    /// The app a window belongs to.
    pub app: Option<SharedString>,
    /// A small picture of it.
    pub thumbnail: Option<Arc<RenderImage>>,
}

impl ShareChoice {
    fn from(source: ShareSource) -> Self {
        Self {
            source: source.id,
            name: source.name.into(),
            app: source.app.map(SharedString::from),
            thumbnail: source.thumbnail.and_then(render_image),
        }
    }
}

impl CallDesk {
    /// What natizap's dialog offers to share, while it asks.
    #[must_use]
    pub fn share_choices(&self) -> Option<ShareChoices> {
        self.call
            .as_ref()
            .and_then(|call| call.video.as_ref())
            .and_then(|video| video.choices.clone())
    }

    /// Shares `source`, chosen in natizap's dialog.
    pub fn share_from(&mut self, source: SourceId, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut() else {
            return;
        };
        let (serial, Going::Live(live)) = (call.serial, &call.going) else {
            return;
        };
        let live = live.clone();
        if let Some(video) = call.video.as_mut()
            && video.choices.take().is_some()
        {
            self.begin(serial, &live, Some(source), cx);
        }
    }

    /// Closes natizap's dialog without sharing anything.
    pub fn cancel_sharing(&mut self, cx: &mut Context<Self>) {
        let live = match self.call.as_ref().map(|call| &call.going) {
            Some(Going::Live(live)) => live.clone(),
            _ => return,
        };
        self.stop_sharing(&live, cx);
    }

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
        if !natizap_asks() {
            return self.begin(serial, live, None, cx);
        }
        video.choices = Some(ShareChoices::Finding);
        // Each source's picture takes a while: never on the UI thread.
        let finding = cx.background_spawn(async { share_sources() });
        cx.spawn(async move |this, cx| {
            let found = finding.await;
            let _ = this.update(cx, |desk, cx| {
                let Some(video) = desk.current(serial).and_then(|call| call.video.as_mut()) else {
                    return;
                };
                // Closed while they were looked for.
                if video.choices.is_some() {
                    let found = found.into_iter().map(ShareChoice::from).collect();
                    video.choices = Some(ShareChoices::Found(found));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Starts capturing `source`, or what the desktop's dialog has the
    /// user choose, off the UI thread: that dialog waits for the user.
    fn begin(
        &mut self,
        serial: u64,
        live: &LiveCall,
        source: Option<SourceId>,
        cx: &mut Context<Self>,
    ) {
        let Some(video) = self.current(serial).and_then(|call| call.video.as_mut()) else {
            return;
        };
        let inset = video.inset.clone();
        let starting = cx.background_spawn(async move { ScreenShare::start(source, inset) });
        let live = live.clone();
        cx.spawn(async move |this, cx| {
            let chosen = starting.await;
            let _ = this.update(cx, |desk, cx| desk.shared(serial, &live, chosen, cx));
        })
        .detach();
    }

    /// The sharing started, or could not.
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
        video.inset.set_lending(true);
        let preview = cx.new(|cx| Screen::new(share.preview(), cx));
        cx.observe(&preview, |_, _, cx| cx.notify()).detach();
        video.shared_preview = Some(preview);
        let sent = video.sent.clone();
        let ending = live.clone();
        video.screen_feeding = Some(cx.spawn(async move |this, cx| {
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

    /// Stops sharing the screen: the camera, if on, sends its own pictures
    /// again, else the call stops sending pictures.
    fn stop_sharing(&mut self, live: &LiveCall, cx: &mut Context<Self>) {
        let Some(video) = self.call.as_mut().and_then(|call| call.video.as_mut()) else {
            return;
        };
        let was_on = video.sharing == Sharing::On;
        video.sharing = Sharing::Off;
        video.choices = None;
        video.inset.set_lending(false);
        video.shared = None;
        video.screen_feeding = None;
        video.shared_preview = None;
        if was_on {
            let told = self.protocol.read(cx).share_screen(live, false, cx);
            logged(
                told,
                "could not tell the other end the screen is no longer shared",
                cx,
            );
        }
        let camera_on = matches!(video.camera, CameraState::On | CameraState::Opening);
        if !camera_on && video.sending {
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
