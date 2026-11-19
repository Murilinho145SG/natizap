//! A call's course from placing or answering it until it leaves the
//! screen: following it as it gets going, steps on and ends, and filing it
//! in the history.

use std::time::{Duration, Instant};

use gpui::{App, AppContext as _, Context, Task};
use natizap_playback::CallAudio;
use natizap_protocol::{
    ActionError, CallEnd, CallOutcome, CallProgress, CallRecord, ChatId, LiveCall, Protocol, Ring,
    Video, Voice,
};

use super::{Call, CallDesk, CallVideo, ENDED_SHOWN, FORMAT, Going, Phase, Reached, voice};
use crate::quality::QualityWatch;

/// A call about to start: who with, which way, since when, and what it
/// sounds and looks like.
pub(super) struct NewCall {
    pub(super) peer: ChatId,
    /// When it was placed or rang, in Unix seconds.
    pub(super) at: i64,
    pub(super) video: Option<CallVideo>,
    pub(super) begin: Begin,
}

/// How a call begins once its sound is open.
pub(super) enum Begin {
    /// Calling someone, with video's ends for a video call.
    Place(Option<Video>),
    /// Answering a call that rings.
    Answer(Ring, Option<Video>),
}

impl Begin {
    fn incoming(&self) -> bool {
        matches!(self, Self::Answer(..))
    }

    /// Asks `protocol` to begin the call with `voice`.
    fn with(
        self,
        peer: &ChatId,
        voice: Voice,
        protocol: &Protocol,
        cx: &App,
    ) -> Task<Result<LiveCall, ActionError>> {
        match self {
            Self::Place(video) => protocol.place_call(peer, voice, video, cx),
            Self::Answer(ring, video) => protocol.answer_call(&ring, voice, video, cx),
        }
    }
}

impl CallDesk {
    /// Puts `call` on screen at once, then opens its sound off the UI
    /// thread, begins it and follows it.
    pub(super) fn start(&mut self, call: NewCall, cx: &mut Context<Self>) {
        let NewCall {
            peer,
            at,
            video,
            begin,
        } = call;
        let incoming = begin.incoming();
        self.serial += 1;
        let serial = self.serial;
        let (devices, calling) = (self.choices.devices.clone(), peer.clone());
        // Detached, not kept with the call: a call hung up before it got
        // going must still be heard getting going, to be ended then.
        cx.spawn(async move |this, cx| {
            // Opening the devices talks to the sound server: not on the UI
            // thread, where it held the call's screen back.
            let opened = cx
                .background_spawn(async move { CallAudio::open(FORMAT, &devices) })
                .await;
            let (audio, ports) = match opened {
                Ok(opened) => opened,
                Err(error) => {
                    tracing::warn!(%error, "no sound for a call");
                    let _ = this.update(cx, |desk, cx| {
                        desk.finish(serial, &CallEnd::Failed(error.to_string()), cx);
                    });
                    return;
                }
            };
            let begun = this.update(cx, |desk, cx| {
                let call = desk
                    .current(serial)
                    .filter(|call| matches!(call.going, Going::Starting))?;
                call.audio = Some(audio);
                Some(begin.with(&calling, voice(ports), desk.protocol.read(cx), cx))
            });
            // Hung up while the sound opened: nothing to begin.
            let Ok(Some(started)) = begun else {
                return;
            };
            let live = match started.await {
                Ok(live) => live,
                Err(error) => {
                    let _ = this.update(cx, |desk, cx| {
                        desk.finish(serial, &CallEnd::Failed(error.to_string()), cx);
                    });
                    return;
                }
            };
            let progress = live.progress();
            let going = this.update(cx, |desk, cx| desk.going(serial, live, cx));
            if !matches!(going, Ok(true)) {
                return;
            }
            while let Ok(step) = progress.recv().await {
                if this
                    .update(cx, |desk, cx| desk.step(serial, step, cx))
                    .is_err()
                {
                    return;
                }
            }
        })
        .detach();
        self.call = Some(Call {
            serial,
            peer,
            incoming,
            phase: Phase::Starting,
            reached: Reached {
                answered: incoming,
                connected: false,
            },
            muted: false,
            going: Going::Starting,
            video,
            video_request: None,
            audio: None,
            at,
            ticking: None,
            quality: QualityWatch::default(),
            judged: None,
            voiced: 0,
            speaking: false,
            leaving: None,
        });
        self.update_ringer(cx);
        cx.notify();
    }

    /// Looks again at how call `serial` goes, a second after the last look:
    /// its connection's quality and whether the other end speaks.
    fn sample(&mut self, serial: u64, cx: &mut Context<Self>) {
        let Some(call) = self.current(serial) else {
            return;
        };
        if let Some(live) = call.going.live() {
            call.judged = call.quality.hear(live.media_counts());
        }
        if let Some(audio) = &call.audio {
            let voiced = audio.voiced_frames();
            call.speaking = voiced > call.voiced;
            call.voiced = voiced;
        }
        cx.notify();
    }

    /// The call under way, if it is the one numbered `serial`.
    pub(super) fn current(&mut self, serial: u64) -> Option<&mut Call> {
        self.call.as_mut().filter(|call| call.serial == serial)
    }

    /// The call got going: whether to follow it on.
    fn going(&mut self, serial: u64, live: LiveCall, cx: &mut Context<Self>) -> bool {
        let Some(call) = self
            .current(serial)
            .filter(|call| matches!(call.going, Going::Starting))
        else {
            // Hung up, or replaced by another, before it got going.
            let hung_up = self.protocol.read(cx).hang_up(&live, cx);
            logged(hung_up, "could not tell the other end the call ended", cx);
            return false;
        };
        let told = live.set_muted(call.muted, cx);
        logged(told, "could not tell the other end of the microphone", cx);
        call.going = Going::Live(live);
        call.phase = if call.incoming {
            Phase::Connecting
        } else {
            Phase::Calling
        };
        if call.video.is_some() {
            self.open_camera(cx);
        }
        self.update_ringer(cx);
        cx.notify();
        true
    }

    fn step(&mut self, serial: u64, step: CallProgress, cx: &mut Context<Self>) {
        let Some(call) = self.current(serial) else {
            return;
        };
        match step {
            CallProgress::Ringing => {
                if !call.reached.answered {
                    call.phase = Phase::Ringing;
                }
            }
            CallProgress::Answered => call.reached.answered = true,
            CallProgress::Connected => call.reached.connected = true,
            CallProgress::Ended(end) => return self.finish(serial, &end, cx),
            CallProgress::PeerVideo(on) => {
                if let Some(video) = call.video.as_mut() {
                    video.peer_on = on;
                }
                return cx.notify();
            }
            CallProgress::VideoRequested(request) => {
                call.video_request = Some(request);
                return cx.notify();
            }
            CallProgress::KeyframeWanted => {
                if let Some(video) = &call.video {
                    video.keyframe_wanted();
                }
                return;
            }
        }
        if call.reached.answered && !matches!(call.phase, Phase::Live { .. }) {
            if call.reached.connected {
                call.phase = Phase::Live {
                    since: Instant::now(),
                };
                call.ticking = Some(tick(serial, cx));
            } else {
                call.phase = Phase::Connecting;
            }
        }
        self.update_ringer(cx);
        cx.notify();
    }

    /// The call ended as `end` says: its sound stops, it goes in the
    /// history, and it leaves the screen shortly after.
    pub(super) fn finish(&mut self, serial: u64, end: &CallEnd, cx: &mut Context<Self>) {
        let log = self.log.clone();
        let Some(call) = self.current(serial) else {
            return;
        };
        if matches!(call.phase, Phase::Ended(_)) {
            return;
        }
        let seconds = match call.phase {
            Phase::Live { since } => u32::try_from(since.elapsed().as_secs()).ok(),
            _ => None,
        };
        call.audio = None;
        // The camera goes off with the call.
        let video = call.video.take().is_some();
        call.video_request = None;
        call.ticking = None;
        call.phase = Phase::Ended(end.clone());
        if let Some(live) = call.going.live() {
            let record = CallRecord {
                id: live.id().to_owned(),
                peer: call.peer.clone(),
                group: None,
                incoming: call.incoming,
                video,
                outcome: match end {
                    CallEnd::Declined => CallOutcome::Declined,
                    _ if call.reached.answered => CallOutcome::Answered,
                    _ => CallOutcome::Missed,
                },
                at: call.at,
                seconds,
            };
            log.read(cx).import(vec![record]);
        }
        let leaving = cx.background_executor().timer(ENDED_SHOWN);
        call.leaving = Some(cx.spawn(async move |this, cx| {
            leaving.await;
            let _ = this.update(cx, |desk, cx| {
                desk.call = None;
                desk.update_ringer(cx);
                cx.notify();
            });
        }));
        self.update_ringer(cx);
        cx.notify();
    }
}

/// Redraws every second, for a live call's clock.
fn tick(serial: u64, cx: &Context<CallDesk>) -> Task<()> {
    let timer = cx.background_executor().clone();
    cx.spawn(async move |this, cx| {
        loop {
            timer.timer(Duration::from_secs(1)).await;
            if this.update(cx, |desk, cx| desk.sample(serial, cx)).is_err() {
                return;
            }
        }
    })
}

/// Lets `task` finish on its own, logging how it failed.
pub(super) fn logged(task: Task<Result<(), ActionError>>, failed: &'static str, cx: &App) {
    cx.background_spawn(async move {
        if let Err(error) = task.await {
            tracing::warn!(%error, "{failed}");
        }
    })
    .detach();
}
