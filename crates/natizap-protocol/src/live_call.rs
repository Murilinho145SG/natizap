//! Calls natizap itself takes part in: placing one, answering or declining
//! one that rings here, hanging up, and following each until it ends.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};

use gpui::{App, AppContext as _, Task};
use whatsapp_rust::RawNodeLease;
use whatsapp_rust::prelude::{Event, EventKind};
use whatsapp_rust::voip::CallHandle;
use whatsapp_rust::voip::audio::{WA_FRAME_SAMPLES, WA_SAMPLE_RATE};
use whatsapp_rust::wacore::types::call::{CallAction, IncomingCall};

use crate::actions::{self, ActionError};
use crate::call_log::CallFollower;
use crate::call_video::{Video, VideoRequest};
use crate::call_watch::Watch;
use crate::calls::Ringing;
use crate::{ChatId, Protocol};

/// Samples a second in a call's sound, which is mono.
pub const VOICE_RATE: u32 = WA_SAMPLE_RATE;
/// Samples in each frame of a call's sound.
pub const VOICE_FRAME: usize = WA_FRAME_SAMPLES;

/// The calls answered on this device, so the ones that end after being
/// answered here are not taken for missed: WhatsApp tells this device
/// nothing about its own answer.
#[derive(Debug, Clone, Default)]
pub(crate) struct AnsweredHere(Arc<Mutex<HashSet<String>>>);

impl AnsweredHere {
    pub(crate) fn contains(&self, call: &str) -> bool {
        self.lock().contains(call)
    }

    pub(crate) fn insert(&self, call: &str) {
        self.lock().insert(call.to_owned());
    }

    fn forget(&self, call: &str) {
        self.lock().remove(call);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashSet<String>> {
        // A set of ids stays whole whatever a panicking holder did.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A call ringing on this device that natizap can answer.
#[derive(Clone)]
pub struct Ring {
    /// WhatsApp's id for the call.
    pub id: String,
    /// Who is calling.
    pub caller: ChatId,
    /// The caller turned their camera on.
    pub video: bool,
    /// When it started ringing, in Unix seconds.
    pub at: i64,
    offer: Arc<IncomingCall>,
}

impl std::fmt::Debug for Ring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ring")
            .field("id", &self.id)
            .field("caller", &self.caller)
            .field("video", &self.video)
            .finish_non_exhaustive()
    }
}

/// A call starting or stopping to ring here.
#[derive(Debug, Clone)]
pub enum RingChange {
    /// A call started ringing.
    Rang(Ring),
    /// The call with this id stopped ringing: the caller gave up, or
    /// another device answered or declined it.
    Stopped(String),
}

/// Tells which calls ring on this device. Feed it every event of
/// [`Rings::EVENTS`].
#[derive(Debug, Clone, Copy, Default)]
pub struct Rings;

impl Rings {
    /// The events calls ring and stop ringing through.
    pub const EVENTS: &[EventKind] = &[
        EventKind::IncomingCall,
        EventKind::MissedCall,
        EventKind::CallEndedElsewhere,
    ];

    /// What `event` says about calls ringing here.
    #[must_use]
    pub fn hear(self, event: &Event) -> Option<RingChange> {
        match event {
            Event::IncomingCall(call) => match &call.action {
                // Group calls and calls replayed from while natizap was
                // offline are not answered here; one without media for
                // this device cannot be.
                CallAction::Offer {
                    call_id,
                    is_video,
                    group_jid: None,
                    ..
                } if !call.offline && call.media().is_some() => Some(RingChange::Rang(Ring {
                    id: call_id.clone(),
                    caller: ChatId::new(&call.from.to_non_ad().to_string()),
                    video: *is_video,
                    at: crate::calls::sent_at(call),
                    offer: Arc::new((**call).clone()),
                })),
                CallAction::Terminate { call_id, .. } => Some(RingChange::Stopped(call_id.clone())),
                _ => None,
            },
            Event::MissedCall(missed) => Some(RingChange::Stopped(missed.call_id.clone())),
            Event::CallEndedElsewhere(ended) => Some(RingChange::Stopped(ended.call_id.clone())),
            _ => None,
        }
    }
}

/// A call's sound, at [`VOICE_RATE`] in frames of [`VOICE_FRAME`]
/// samples.
#[derive(Debug, Clone)]
pub struct Voice {
    /// Frames heard by the microphone, to send.
    pub microphone: async_channel::Receiver<Vec<i16>>,
    /// Where frames received go to be played.
    pub speaker: async_channel::Sender<Vec<i16>>,
    /// Where standard Opus packets received go to be decoded and played:
    /// the other end's phone may send Opus instead of the call's own
    /// codec, which only it can tell.
    pub opus: async_channel::Sender<Vec<u8>>,
}

/// How far a call has come.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallProgress {
    /// The other end's phone is ringing.
    Ringing,
    /// The other end picked up.
    Answered,
    /// Sound flows between both ends.
    Connected,
    /// The other end's camera came on (`true`) or went off.
    PeerVideo(bool),
    /// The other end asks to turn the call into a video call: answer with
    /// [`Protocol::accept_video`].
    VideoRequested(VideoRequest),
    /// The other end lost pictures and needs a keyframe to go on.
    KeyframeWanted,
    /// The other end turned their microphone off (`false`) or back on.
    PeerMicrophone(bool),
    /// The call is over.
    Ended(CallEnd),
}

/// How a call ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallEnd {
    /// It was answered and someone hung up.
    Finished,
    /// The other end turned it down.
    Declined,
    /// Nobody picked up.
    Unanswered,
    /// The connection could not be made or was lost.
    Failed(String),
}

/// What a call's media counted since it started.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MediaCounts {
    /// Frames of the other end's sound that arrived and played.
    pub decoded: u32,
    /// Frames that never arrived, or late, and were made up to cover the
    /// gap.
    pub concealed: u32,
}

/// A call natizap takes part in. Clones control the same call.
#[derive(Clone)]
pub struct LiveCall {
    pub(crate) handle: CallHandle,
    progress: async_channel::Receiver<CallProgress>,
}

impl std::fmt::Debug for LiveCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveCall")
            .field("id", &self.handle.call_id())
            .finish_non_exhaustive()
    }
}

impl LiveCall {
    /// WhatsApp's id for the call.
    #[must_use]
    pub fn id(&self) -> &str {
        self.handle.call_id()
    }

    /// Stops or resumes sending the microphone's sound, and tells the
    /// other end. Muting takes effect even when the other end cannot be
    /// told; unmuting waits until it was.
    pub fn set_muted(&self, muted: bool, cx: &App) -> Task<Result<(), ActionError>> {
        let handle = self.handle.clone();
        actions::on_tokio(cx, async move {
            handle
                .set_muted(muted)
                .await
                .map_err(|error| ActionError::refused(&error))
        })
    }

    /// What the call's media has counted so far, to tell how well the
    /// connection carries it.
    #[must_use]
    pub fn media_counts(&self) -> MediaCounts {
        let stats = self.handle.media_stats();
        MediaCounts {
            decoded: stats.audio_frames_decoded,
            concealed: stats.audio_frames_concealed,
        }
    }

    /// How the call goes, step by step, ending with
    /// [`CallProgress::Ended`]. Clones share one queue: read it in one
    /// place.
    #[must_use]
    pub fn progress(&self) -> async_channel::Receiver<CallProgress> {
        self.progress.clone()
    }
}

impl Protocol {
    /// Follows calls into missed ones, knowing which this device answered.
    #[must_use]
    pub fn ringing(&self) -> Ringing {
        Ringing::new(self.answered.clone())
    }

    /// Follows calls into the call history, knowing which this device
    /// answered.
    #[must_use]
    pub fn call_follower(&self) -> CallFollower {
        CallFollower::new(self.answered.clone())
    }

    /// Calls `peer`, with `voice` as the call's sound and, for a video
    /// call, `video` as its pictures.
    pub fn place_call(
        &self,
        peer: &ChatId,
        voice: Voice,
        video: Option<Video>,
        cx: &App,
    ) -> Task<Result<LiveCall, ActionError>> {
        let to = match actions::jid(peer) {
            Ok(to) => to,
            Err(error) => return Task::ready(Err(error)),
        };
        let signals = self.subscribe(&[EventKind::IncomingCall, EventKind::RawNode]);
        let opus = voice.opus.clone();
        let video = video.map(|video| video.endpoints(cx));
        let started = self.connection.run(cx, move |client| async move {
            let voip = client.voip();
            let call = voip.call(&to).audio(voice.microphone, voice.speaker);
            let call = match video {
                Some((camera, screen)) => call.video(camera, screen),
                None => call,
            };
            let handle = call
                .start()
                .await
                .map_err(|error| ActionError::refused(&error))?;
            Ok((handle, client.acquire_raw_node_forwarding()))
        });
        follow(started, signals, opus, false, cx)
    }

    /// Answers `ring`, with `voice` as the call's sound and, when the
    /// caller turned their camera on, `video` as its pictures.
    pub fn answer_call(
        &self,
        ring: &Ring,
        voice: Voice,
        video: Option<Video>,
        cx: &App,
    ) -> Task<Result<LiveCall, ActionError>> {
        let signals = self.subscribe(&[EventKind::IncomingCall, EventKind::RawNode]);
        let opus = voice.opus.clone();
        // Before answering: the caller may hang up before the answer
        // returns, and that call was not missed either.
        self.answered.insert(&ring.id);
        let (answered, id, offer) = (
            self.answered.clone(),
            ring.id.clone(),
            Arc::clone(&ring.offer),
        );
        // An offer without video takes none: the library refuses it.
        let video = video
            .filter(|_| ring.video)
            .map(|video| video.endpoints(cx));
        let started = self.connection.run(cx, move |client| async move {
            let voip = client.voip();
            let answer = voip.accept(&offer).audio(voice.microphone, voice.speaker);
            let answer = match video {
                Some((camera, screen)) => answer.video(camera, screen),
                None => answer,
            };
            // The answer's setup is large: on the heap, not the stack.
            let started = Box::pin(answer.start()).await;
            if started.is_err() {
                answered.forget(&id);
            }
            let handle = started.map_err(|error| ActionError::refused(&error))?;
            Ok((handle, client.acquire_raw_node_forwarding()))
        });
        follow(started, signals, opus, true, cx)
    }

    /// Turns `ring` down.
    pub fn decline_call(&self, ring: &Ring, cx: &App) -> Task<Result<(), ActionError>> {
        let offer = Arc::clone(&ring.offer);
        self.connection.run(cx, move |client| async move {
            client
                .voip()
                .reject(&offer)
                .await
                .map_err(|error| ActionError::refused(&error))
        })
    }

    /// Ends `call`, telling the other end. Its sound stops even when the
    /// other end cannot be told.
    pub fn hang_up(&self, call: &LiveCall, cx: &App) -> Task<Result<(), ActionError>> {
        let handle = call.handle.clone();
        let told = self.connection.run(cx, {
            let handle = handle.clone();
            move |client| async move {
                client
                    .voip()
                    .terminate(handle.call_id(), &handle.peer_jid(), handle.call_creator())
                    .await
                    .map_err(|error| ActionError::refused(&error))
            }
        });
        cx.background_spawn(async move {
            let told = told.await;
            handle.hangup_local().await;
            told
        })
    }
}

/// Once `started` gives a call, follows it through `signals`, its media's
/// events and its end. `started` also gives what has the client hand over
/// the raw signaling the call's watch reads the other end's microphone
/// from, kept for as long as the watch runs.
fn follow(
    started: Task<Result<(CallHandle, RawNodeLease), ActionError>>,
    signals: async_channel::Receiver<Arc<Event>>,
    opus: async_channel::Sender<Vec<u8>>,
    answered: bool,
    cx: &App,
) -> Task<Result<LiveCall, ActionError>> {
    let executor = cx.background_executor().clone();
    cx.background_spawn(async move {
        let (handle, raw) = started.await?;
        let (tell, progress) = async_channel::unbounded();
        let watch = Watch::new(handle.call_id(), answered);
        let watching = watch.run(handle.clone(), signals, opus, tell);
        executor
            .spawn(async move {
                watching.await;
                drop(raw);
            })
            .detach();
        Ok(LiveCall { handle, progress })
    })
}

#[cfg(test)]
mod tests {
    use whatsapp_rust::Jid;

    use super::*;

    #[test]
    fn a_call_hung_up_elsewhere_stops_ringing() {
        let caller = Jid::pn("5511912345678");
        let terminate = Event::IncomingCall(Box::new(
            IncomingCall::builder()
                .from(caller.clone())
                .stanza_id("stanza".into())
                .timestamp(whatsapp_rust::time::now_utc())
                .offline(false)
                .action(CallAction::Terminate {
                    call_id: "CALL1".into(),
                    call_creator: caller,
                    reason: None,
                    duration: None,
                    audio_duration: None,
                })
                .build(),
        ));
        let stopped = Rings.hear(&terminate);
        assert!(matches!(stopped, Some(RingChange::Stopped(id)) if id == "CALL1"));
    }
}
