//! The call natizap is in and the calls ringing on it: placing, answering,
//! declining and hanging up, the call's sound, its ringing, and its place
//! in the history once it ends.

use std::time::{Duration, Instant};

use gpui::{
    AppContext as _, Context, Entity, EventEmitter, SharedString, SystemNotification, Task,
};
use natizap_playback::{CallAudio, CallFormat, CallPorts};
use natizap_protocol::{
    ActionError, CallEnd, CallOutcome, CallProgress, CallRecord, ChatId, LiveCall, Protocol, Ring,
    RingChange, Rings, VOICE_FRAME, VOICE_RATE, Voice,
};

use crate::log::CallLog;
use crate::ringtone::Ringtone;

/// How long an ended call stays on screen, saying how it ended.
const ENDED_SHOWN: Duration = Duration::from_millis(2_500);

/// The sound a call takes.
const FORMAT: CallFormat = CallFormat {
    rate: VOICE_RATE,
    frame: VOICE_FRAME,
};

/// A call started ringing here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallRang {
    /// Who is calling.
    pub caller: ChatId,
}

/// Where a call is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// Getting the call going.
    Starting,
    /// Placed, waiting for the other phone to ring.
    Calling,
    /// The other phone rings.
    Ringing,
    /// Answered, the sound not flowing yet.
    Connecting,
    /// Under way since `since`.
    Live {
        /// When the two ends started hearing each other.
        since: Instant,
    },
    /// Over, and how.
    Ended(CallEnd),
}

/// The call on screen, as views show it.
#[derive(Debug, Clone)]
pub struct CallShown {
    /// Who the call is with.
    pub peer: ChatId,
    /// What they are called.
    pub title: SharedString,
    /// They called.
    pub incoming: bool,
    pub phase: Phase,
    /// The microphone is silenced.
    pub muted: bool,
}

/// The one call natizap is in, from placing or answering it until shortly
/// after it ends.
struct Call {
    /// Tells this call from the ones before it, whose news may still come.
    serial: u64,
    peer: ChatId,
    incoming: bool,
    phase: Phase,
    reached: Reached,
    muted: bool,
    going: Going,
    /// The microphone and speaker, open while the call is.
    audio: Option<CallAudio>,
    /// When it was placed or rang, in Unix seconds.
    at: i64,
    /// Ticks the call's clock while it is live.
    ticking: Option<Task<()>>,
    /// Takes the call off the screen once it ended.
    leaving: Option<Task<()>>,
}

/// How far a call got: the two need not come in order.
#[derive(Debug, Clone, Copy, Default)]
struct Reached {
    /// Someone picked up.
    answered: bool,
    /// Sound can flow.
    connected: bool,
}

/// Whether the call got going.
enum Going {
    /// Still being placed or answered.
    Starting,
    /// Hung up before it got going: it ends as soon as it does.
    Abandoned,
    Live(LiveCall),
}

impl Going {
    fn live(&self) -> Option<&LiveCall> {
        match self {
            Self::Live(live) => Some(live),
            Self::Starting | Self::Abandoned => None,
        }
    }
}

/// Calls: the one under way and the ones ringing.
pub struct CallDesk {
    protocol: Entity<Protocol>,
    log: Entity<CallLog>,
    rings: Vec<Ring>,
    call: Option<Call>,
    /// The serial the next call gets.
    serial: u64,
    /// The ringing sound, while something rings.
    ringer: Option<(Ringtone, Task<()>)>,
    _hearing: Task<()>,
}

impl std::fmt::Debug for CallDesk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallDesk")
            .field("rings", &self.rings)
            .field("in_call", &self.call.is_some())
            .finish_non_exhaustive()
    }
}

impl EventEmitter<CallRang> for CallDesk {}

impl CallDesk {
    /// Takes calls through `protocol`, filing the ones made or answered
    /// here in `log`.
    pub fn new(protocol: Entity<Protocol>, log: Entity<CallLog>, cx: &mut Context<Self>) -> Self {
        let events = protocol.read(cx).subscribe(Rings::EVENTS);
        let hearing = cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                let Some(change) = Rings.hear(&event) else {
                    continue;
                };
                if this.update(cx, |desk, cx| desk.ring(change, cx)).is_err() {
                    break;
                }
            }
        });
        Self {
            protocol,
            log,
            rings: Vec::new(),
            call: None,
            serial: 0,
            ringer: None,
            _hearing: hearing,
        }
    }

    /// The call under way, or just ended.
    #[must_use]
    pub fn call(&self, cx: &gpui::App) -> Option<CallShown> {
        let call = self.call.as_ref()?;
        Some(CallShown {
            peer: call.peer.clone(),
            title: self.log.read(cx).title(&call.peer),
            incoming: call.incoming,
            phase: call.phase.clone(),
            muted: call.muted,
        })
    }

    /// The call ringing longest, to answer or decline.
    #[must_use]
    pub fn ringing(&self) -> Option<&Ring> {
        self.rings.first()
    }

    /// What `chat` is called.
    #[must_use]
    pub fn title(&self, chat: &ChatId, cx: &gpui::App) -> SharedString {
        self.log.read(cx).title(chat)
    }

    /// Calls `peer`, unless a call is under way already.
    pub fn place(&mut self, peer: ChatId, cx: &mut Context<Self>) {
        if self.call.is_some() {
            return;
        }
        let at = now();
        let (audio, started) = match CallAudio::open(FORMAT) {
            Ok((audio, ports)) => {
                let started = self.protocol.read(cx).place_call(&peer, voice(ports), cx);
                (Some(audio), Some(started))
            }
            Err(error) => {
                tracing::warn!(%error, "no sound for a call");
                (None, None)
            }
        };
        self.start(peer, false, at, audio, started, cx);
    }

    /// Answers the call ringing longest. A call under way ends first.
    pub fn answer(&mut self, cx: &mut Context<Self>) {
        let Some(ring) = self.rings.first().cloned() else {
            return;
        };
        self.rings.retain(|other| other.id != ring.id);
        Self::dismiss_notification(&ring, cx);
        // One call at a time: the one under way ends, filed as it should.
        self.hang_up(cx);
        self.call = None;
        let (audio, started) = match CallAudio::open(FORMAT) {
            Ok((audio, ports)) => {
                let started = self.protocol.read(cx).answer_call(&ring, voice(ports), cx);
                (Some(audio), Some(started))
            }
            Err(error) => {
                tracing::warn!(%error, "no sound for a call");
                (None, None)
            }
        };
        self.start(ring.caller, true, ring.at, audio, started, cx);
    }

    /// Turns down the call ringing longest.
    pub fn decline(&mut self, cx: &mut Context<Self>) {
        let Some(ring) = self.rings.first().cloned() else {
            return;
        };
        self.rings.retain(|other| other.id != ring.id);
        Self::dismiss_notification(&ring, cx);
        let declined = self.protocol.read(cx).decline_call(&ring, cx);
        logged(declined, "could not decline the call", cx);
        self.update_ringer(cx);
        cx.notify();
    }

    /// Ends the call under way.
    pub fn hang_up(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut() else {
            return;
        };
        if matches!(call.phase, Phase::Ended(_)) {
            return;
        }
        let end = if call.reached.answered {
            CallEnd::Finished
        } else {
            CallEnd::Unanswered
        };
        match call.going.live().cloned() {
            Some(live) => {
                let hung_up = self.protocol.read(cx).hang_up(&live, cx);
                logged(hung_up, "could not tell the other end the call ended", cx);
            }
            // It ends as soon as it gets going.
            None => call.going = Going::Abandoned,
        }
        let serial = call.serial;
        self.finish(serial, &end, cx);
    }

    /// Silences the microphone, or lets it be heard again.
    pub fn toggle_muted(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut() else {
            return;
        };
        call.muted = !call.muted;
        if let Some(live) = call.going.live() {
            live.set_muted(call.muted);
        }
        cx.notify();
    }

    fn start(
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
    fn finish(&mut self, serial: u64, end: &CallEnd, cx: &mut Context<Self>) {
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

    fn ring(&mut self, change: RingChange, cx: &mut Context<Self>) {
        match change {
            RingChange::Rang(ring) => {
                if self.rings.iter().any(|other| other.id == ring.id) {
                    return;
                }
                let title = self.title(&ring.caller, cx);
                cx.show_system_notification(SystemNotification {
                    tag: ring.caller.as_str().to_owned().into(),
                    title,
                    body: if ring.video {
                        "Incoming video call".into()
                    } else {
                        "Incoming voice call".into()
                    },
                    actions: Vec::new(),
                });
                cx.emit(CallRang {
                    caller: ring.caller.clone(),
                });
                self.rings.push(ring);
            }
            RingChange::Stopped(id) => {
                let Some(ring) = self.rings.iter().find(|ring| ring.id == id).cloned() else {
                    return;
                };
                Self::dismiss_notification(&ring, cx);
                self.rings.retain(|ring| ring.id != id);
            }
        }
        self.update_ringer(cx);
        cx.notify();
    }

    fn dismiss_notification(ring: &Ring, cx: &gpui::App) {
        // Dismissing goes on even when the task is dropped.
        drop(cx.dismiss_system_notification(ring.caller.as_str()));
    }

    /// Rings when a call comes in with none under way, or while the other
    /// phone rings; is quiet otherwise.
    fn update_ringer(&mut self, cx: &Context<Self>) {
        let wanted = match self.call.as_ref().map(|call| &call.phase) {
            None if !self.rings.is_empty() => Some(Ringtone::Incoming),
            Some(Phase::Ringing) => Some(Ringtone::Ringback),
            _ => None,
        };
        if self.ringer.as_ref().map(|(tone, _)| *tone) == wanted {
            return;
        }
        self.ringer = wanted.map(|tone| (tone, tone.ring(cx)));
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
fn logged(task: Task<Result<(), ActionError>>, failed: &'static str, cx: &gpui::App) {
    cx.background_spawn(async move {
        if let Err(error) = task.await {
            tracing::warn!(%error, "{failed}");
        }
    })
    .detach();
}

fn voice(ports: CallPorts) -> Voice {
    Voice {
        microphone: ports.microphone,
        speaker: ports.speaker,
        opus: ports.opus,
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
        })
}
