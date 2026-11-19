//! The call natizap is in and the calls ringing on it: placing, answering,
//! declining and hanging up, the call's sound, its ringing, and its place
//! in the history once it ends.

use std::time::{Duration, Instant};

use gpui::{AppContext as _, Context, Entity, EventEmitter, RenderImage, SharedString, Task};
use natizap_playback::{CallAudio, CallDevices, CallFormat, CallPorts};
use natizap_protocol::{
    CallEnd, ChatId, LiveCall, Protocol, Ring, Rings, VOICE_FRAME, VOICE_RATE, VideoRequest, Voice,
};

use std::path::PathBuf;
use std::sync::Arc;

use crate::choices::Choices;
use crate::log::CallLog;
use crate::quality::{Quality, QualityWatch};
use crate::ringtone::{Ringing, Ringtone};

mod layout;
mod lifecycle;
mod ringing;
mod sharing;
mod video;

use layout::Arrangement;
pub use layout::CallLayout;
pub use sharing::{ShareChoice, ShareChoices, Sharing};
use video::CallVideo;
pub use video::CameraState;

use lifecycle::{Begin, NewCall, logged};

/// How long an ended call stays on screen, saying how it ended.
const ENDED_SHOWN: Duration = Duration::from_millis(2_500);

/// The sound a call takes.
const FORMAT: CallFormat = CallFormat {
    rate: VOICE_RATE,
    frame: VOICE_FRAME,
};

/// A kind of device calls use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Microphone,
    Speaker,
    Camera,
}

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
    pub phase: Phase,
    /// The microphone is silenced.
    pub muted: bool,
    /// The call's video, once it has some.
    pub video: Option<VideoShown>,
    /// The other end asks to turn the call into a video call.
    pub video_requested: bool,
    /// How well the connection carries the call, once it is live.
    pub quality: Option<Quality>,
    /// The other end spoke in the last second.
    pub speaking: bool,
}

/// A call's video, as views show it.
#[derive(Debug, Clone)]
pub struct VideoShown {
    /// The other end's newest picture, while their camera is on.
    pub remote: Option<Arc<RenderImage>>,
    /// The camera's newest picture, while it is on.
    pub camera_picture: Option<Arc<RenderImage>>,
    /// The shared screen's newest picture, while it is shared.
    pub screen_picture: Option<Arc<RenderImage>>,
    pub camera: CameraState,
    /// Whether the screen is shared.
    pub sharing: Sharing,
}

impl VideoShown {
    /// What the user sends, as they see it small: the shared screen, else
    /// the camera.
    #[must_use]
    pub fn own_picture(&self) -> Option<Arc<RenderImage>> {
        self.screen_picture
            .clone()
            .or_else(|| self.camera_picture.clone())
    }
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
    /// The call's video, once it has some.
    video: Option<CallVideo>,
    /// The other end asks to turn the call into a video call.
    video_request: Option<VideoRequest>,
    /// The microphone and speaker, open while the call is.
    audio: Option<CallAudio>,
    /// When it was placed or rang, in Unix seconds.
    at: i64,
    /// Ticks the call's clock while it is live.
    ticking: Option<Task<()>>,
    /// Judges the connection while the call is live.
    quality: QualityWatch,
    judged: Option<Quality>,
    /// The other end's voiced frames at the last look, and whether more
    /// came since.
    voiced: u64,
    speaking: bool,
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
    ringer: Option<(Ringing, Task<()>)>,
    /// The ringtone and devices chosen.
    choices: Choices,
    /// Where the choices are kept.
    settings: PathBuf,
    /// A ringtone being tried in the settings.
    preview: Option<Task<()>>,
    /// How the call shows in the main window.
    arrangement: Arrangement,
    _tasks: [Task<()>; 2],
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
    /// here in `log`, ringing with the ringtone chosen in the file at
    /// `settings`.
    pub fn new(
        protocol: Entity<Protocol>,
        log: Entity<CallLog>,
        settings: PathBuf,
        cx: &mut Context<Self>,
    ) -> Self {
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
        let reading = settings.clone();
        let loading = cx.spawn(async move |this, cx| {
            let choices = cx
                .background_spawn(async move { Choices::read(&reading) })
                .await;
            let _ = this.update(cx, |desk, cx| {
                desk.choices = choices;
                cx.notify();
            });
        });
        Self {
            protocol,
            log,
            rings: Vec::new(),
            call: None,
            serial: 0,
            ringer: None,
            choices: Choices::default(),
            settings,
            preview: None,
            arrangement: Arrangement::default(),
            _tasks: [hearing, loading],
        }
    }

    /// What calls coming in ring with.
    #[must_use]
    pub fn ringtone(&self) -> Ringtone {
        self.choices.ringtone
    }

    /// The devices calls use.
    #[must_use]
    pub fn devices(&self) -> &CallDevices {
        &self.choices.devices
    }

    /// Uses the device `key` names, or the system's own for `None`, as the
    /// `kind` of device calls use from now on, the call under way included.
    pub fn set_device(&mut self, kind: DeviceKind, key: Option<String>, cx: &mut Context<Self>) {
        let devices = &mut self.choices.devices;
        match kind {
            DeviceKind::Microphone => devices.microphone = key,
            DeviceKind::Speaker => devices.speaker = key,
            DeviceKind::Camera => devices.camera = key,
        }
        self.choices.write(self.settings.clone(), cx);
        let devices = self.choices.devices.clone();
        if let Some(call) = self.call.as_mut() {
            match kind {
                DeviceKind::Microphone | DeviceKind::Speaker => {
                    if let Some(audio) = call.audio.as_mut()
                        && let Err(error) = audio.switch(&devices)
                    {
                        tracing::warn!(%error, "the call keeps the devices it had");
                    }
                }
                DeviceKind::Camera => self.reopen_camera(cx),
            }
        }
        cx.notify();
    }

    /// Rings with `ringtone` from now on, keeping the choice and playing
    /// it once to hear.
    pub fn set_ringtone(&mut self, ringtone: Ringtone, cx: &mut Context<Self>) {
        self.choices.ringtone = ringtone;
        self.choices.write(self.settings.clone(), cx);
        self.preview = Some(Ringing::Incoming(ringtone).play_once(cx));
        self.update_ringer(cx);
        cx.notify();
    }

    /// The call under way, or just ended.
    #[must_use]
    pub fn call(&self, cx: &gpui::App) -> Option<CallShown> {
        let call = self.call.as_ref()?;
        Some(CallShown {
            peer: call.peer.clone(),
            title: self.log.read(cx).title(&call.peer),
            phase: call.phase.clone(),
            muted: call.muted,
            video: call.video.as_ref().map(|video| VideoShown {
                remote: video
                    .peer_on
                    .then(|| video.remote.read(cx).image())
                    .flatten(),
                camera_picture: video
                    .preview
                    .as_ref()
                    .and_then(|preview| preview.read(cx).image()),
                screen_picture: video
                    .shared_preview
                    .as_ref()
                    .and_then(|preview| preview.read(cx).image()),
                camera: video.camera,
                sharing: video.sharing,
            }),
            video_requested: call.video_request.is_some(),
            quality: call.judged,
            speaking: call.speaking,
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

    /// Calls `peer`, with the camera on for a `video` call, unless a call
    /// is under way already.
    pub fn place(&mut self, peer: ChatId, video: bool, cx: &mut Context<Self>) {
        if self.call.is_some() {
            return;
        }
        let at = now();
        let (video, ends) = if video {
            Self::video_for_call(cx).unzip()
        } else {
            (None, None)
        };
        let call = NewCall {
            peer,
            at,
            video,
            begin: Begin::Place(ends),
        };
        self.start(call, cx);
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
        self.arrangement.full_screen = false;
        // A video call is answered with the camera on, as the phone does.
        let (video, ends) = if ring.video {
            Self::video_for_call(cx).unzip()
        } else {
            (None, None)
        };
        let call = NewCall {
            peer: ring.caller.clone(),
            at: ring.at,
            video,
            begin: Begin::Answer(ring, ends),
        };
        self.start(call, cx);
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
            let told = live.set_muted(call.muted, cx);
            logged(told, "could not tell the other end of the microphone", cx);
        }
        cx.notify();
    }
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
