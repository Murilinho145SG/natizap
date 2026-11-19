//! A call's course from placing or answering it until it leaves the
//! screen: following it as it gets going, steps on and ends, and filing it
//! in the history.

use std::time::{Duration, Instant};

use gpui::{AppContext as _, Context, Task};
use natizap_playback::CallAudio;
use natizap_protocol::{
    ActionError, CallEnd, CallOutcome, CallProgress, CallRecord, ChatId, LiveCall,
};

use super::{Call, CallDesk, ENDED_SHOWN, Going, Phase, Reached};

impl CallDesk {
    pub(super) fn start(
        &mut self,
        peer: ChatId,
        incoming: bool,
        at: i64,
        audio: Option<CallAudio>,
        started: Option<Task<Result<LiveCall, ActionError>>>,
        cx: &mut Context<Self>,
    ) {
        self.serial += 1;
        let serial = self.serial;
        // Detached, not kept with the call: a call hung up before it got
        // going must still be heard getting going, to be ended then.
        cx.spawn(async move |this, cx| {
            let Some(started) = started else {
                let _ = this.update(cx, |desk, cx| {
                    desk.finish(
                        serial,
                        &CallEnd::Failed("no microphone or speaker".into()),
                        cx,
                    );
                });
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
            audio,
            at,
            ticking: None,
            leaving: None,
        });
        self.update_ringer(cx);
        cx.notify();
    }

    /// The call under way, if it is the one numbered `serial`.
    fn current(&mut self, serial: u64) -> Option<&mut Call> {
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
        live.set_muted(call.muted);
        call.going = Going::Live(live);
        call.phase = if call.incoming {
            Phase::Connecting
        } else {
            Phase::Calling
        };
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
            // Video comes with video calls, which this desk does not take yet.
            CallProgress::PeerVideo(_)
            | CallProgress::VideoRequested(_)
            | CallProgress::KeyframeWanted => return,
        }
        if call.reached.answered && !matches!(call.phase, Phase::Live { .. }) {
            if call.reached.connected {
                call.phase = Phase::Live {
                    since: Instant::now(),
                };
                call.ticking = Some(tick(cx));
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
        call.ticking = None;
        call.phase = Phase::Ended(end.clone());
        if let Some(live) = call.going.live() {
            let record = CallRecord {
                id: live.id().to_owned(),
                peer: call.peer.clone(),
                group: None,
                incoming: call.incoming,
                video: false,
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
fn tick(cx: &Context<CallDesk>) -> Task<()> {
    let timer = cx.background_executor().clone();
    cx.spawn(async move |this, cx| {
        loop {
            timer.timer(Duration::from_secs(1)).await;
            if this.update(cx, |_, cx| cx.notify()).is_err() {
                return;
            }
        }
    })
}

/// Lets `task` finish on its own, logging how it failed.
pub(super) fn logged(task: Task<Result<(), ActionError>>, failed: &'static str, cx: &gpui::App) {
    cx.background_spawn(async move {
        if let Err(error) = task.await {
            tracing::warn!(%error, "{failed}");
        }
    })
    .detach();
}
