//! Calls natizap itself takes part in: placing one, answering or declining
//! one that rings here, hanging up, and following each until it ends.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};

use futures::StreamExt as _;
use gpui::{App, AppContext as _, Task};
use whatsapp_rust::prelude::{Event, EventKind};
use whatsapp_rust::voip::audio::{WA_FRAME_SAMPLES, WA_SAMPLE_RATE};
use whatsapp_rust::voip::{CallEvent, CallHandle, depacketize_opus_from_mlow};
use whatsapp_rust::wacore::types::call::{CallAction, IncomingCall};

use crate::actions::{self, ActionError};
use crate::call_log::CallFollower;
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
                } if !call.offline && call.media.is_some() => Some(RingChange::Rang(Ring {
                    id: call_id.clone(),
                    caller: ChatId::new(&call.from.to_non_ad().to_string()),
                    video: *is_video,
                    at: call.timestamp.timestamp(),
                    offer: Arc::new(call.clone()),
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

/// A call natizap takes part in. Clones control the same call.
#[derive(Clone)]
pub struct LiveCall {
    handle: CallHandle,
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

    /// Stops or resumes sending the microphone's sound.
    pub fn set_muted(&self, muted: bool) {
        self.handle.set_muted(muted);
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

    /// Calls `peer`, with `voice` as the call's sound.
    pub fn place_call(
        &self,
        peer: &ChatId,
        voice: Voice,
        cx: &App,
    ) -> Task<Result<LiveCall, ActionError>> {
        let to = match actions::jid(peer) {
            Ok(to) => to,
            Err(error) => return Task::ready(Err(error)),
        };
        let signals = self.subscribe(&[EventKind::IncomingCall]);
        let opus = voice.opus.clone();
        let started = self.connection.run(cx, move |client| async move {
            client
                .voip()
                .call(&to)
                .audio(voice.microphone, voice.speaker)
                .start()
                .await
                .map_err(|error| ActionError::refused(&error))
        });
        follow(started, signals, opus, false, cx)
    }

    /// Answers `ring`, with `voice` as the call's sound.
    pub fn answer_call(
        &self,
        ring: &Ring,
        voice: Voice,
        cx: &App,
    ) -> Task<Result<LiveCall, ActionError>> {
        let signals = self.subscribe(&[EventKind::IncomingCall]);
        let opus = voice.opus.clone();
        // Before answering: the caller may hang up before the answer
        // returns, and that call was not missed either.
        self.answered.insert(&ring.id);
        let (answered, id, offer) = (
            self.answered.clone(),
            ring.id.clone(),
            Arc::clone(&ring.offer),
        );
        let started = self.connection.run(cx, move |client| async move {
            // The answer's setup is large: on the heap, not the stack.
            let started = Box::pin(
                client
                    .voip()
                    .accept(&offer)
                    .audio(voice.microphone, voice.speaker)
                    .start(),
            )
            .await;
            if started.is_err() {
                answered.forget(&id);
            }
            started.map_err(|error| ActionError::refused(&error))
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
            handle.hangup().await;
            told
        })
    }
}

/// Once `started` gives a call, follows it through `signals`, its media's
/// events and its end.
fn follow(
    started: Task<Result<CallHandle, ActionError>>,
    signals: async_channel::Receiver<Arc<Event>>,
    opus: async_channel::Sender<Vec<u8>>,
    answered: bool,
    cx: &App,
) -> Task<Result<LiveCall, ActionError>> {
    let executor = cx.background_executor().clone();
    cx.background_spawn(async move {
        let handle = started.await?;
        let (tell, progress) = async_channel::unbounded();
        let watch = Watch::new(handle.call_id(), answered);
        executor
            .spawn(watch.run(handle.clone(), signals, opus, tell))
            .detach();
        Ok(LiveCall { handle, progress })
    })
}

/// Something a call's watch heard.
enum Heard {
    Signal(Arc<Event>),
    Media(CallEvent),
    Ended,
}

/// Follows one call into [`CallProgress`].
struct Watch {
    id: String,
    answered: bool,
    /// How it ends, once something said so.
    end: Option<CallEnd>,
}

impl Watch {
    fn new(id: &str, answered: bool) -> Self {
        Self {
            id: id.to_owned(),
            answered,
            end: None,
        }
    }

    async fn run(
        mut self,
        handle: CallHandle,
        signals: async_channel::Receiver<Arc<Event>>,
        opus: async_channel::Sender<Vec<u8>>,
        tell: async_channel::Sender<CallProgress>,
    ) {
        let ending = handle.clone();
        let heard = futures::stream::select(
            futures::stream::select(
                signals.map(Heard::Signal),
                handle.events().map(Heard::Media),
            ),
            futures::stream::once(async move {
                ending.wait_ended().await;
                Heard::Ended
            }),
        );
        futures::pin_mut!(heard);
        // What went by, told in the log once the call ends: where a call's
        // voice got lost is otherwise invisible.
        let (mut foreign, mut unwrapped) = (0_u64, 0_u64);
        while let Some(heard) = heard.next().await {
            match &heard {
                Heard::Media(CallEvent::ForeignAudio(packet)) => {
                    foreign += 1;
                    if let Some(packet) = standard_opus(packet) {
                        unwrapped += 1;
                        // Full means the speaker is behind: this piece is lost.
                        let _ = opus.try_send(packet);
                    }
                    continue;
                }
                // Several a second: too many for the log.
                Heard::Media(CallEvent::RtcpReceived { .. }) | Heard::Signal(_) | Heard::Ended => {}
                Heard::Media(event) => tracing::info!(call = %self.id, ?event, "call media event"),
            }
            let Some(progress) = self.hear(&heard) else {
                continue;
            };
            let ended = matches!(progress, CallProgress::Ended(_));
            if ended {
                tracing::info!(
                    call = %self.id,
                    foreign,
                    unwrapped,
                    "call ended: Opus packets from the other end, and how many unwrapped"
                );
            }
            // Nobody listening is no reason to stop following: the call
            // runs on until it ends.
            let _ = tell.send(progress).await;
            if ended {
                return;
            }
        }
    }

    fn hear(&mut self, heard: &Heard) -> Option<CallProgress> {
        match heard {
            Heard::Signal(event) => {
                let Event::IncomingCall(call) = event.as_ref() else {
                    return None;
                };
                if call.action.call_id() != self.id {
                    return None;
                }
                self.signal(&call.action)
            }
            Heard::Media(event) => {
                match event {
                    CallEvent::RelayAllocated => return Some(CallProgress::Connected),
                    CallEvent::RelayAllocateFailed(code) => {
                        self.fail(format!("WhatsApp's relay refused the call ({code})"));
                    }
                    CallEvent::RelayAllocateTimedOut | CallEvent::RelayReconnectTimedOut => {
                        self.fail("the connection to the call was lost".to_owned());
                    }
                    CallEvent::AudioFormatMismatch { .. } => {
                        self.fail("the other end's sound is in a format natizap lacks".to_owned());
                    }
                    _ => {}
                }
                None
            }
            Heard::Ended => {
                let end = self.end.take().unwrap_or(if self.answered {
                    CallEnd::Finished
                } else {
                    CallEnd::Unanswered
                });
                Some(CallProgress::Ended(end))
            }
        }
    }

    fn signal(&mut self, action: &CallAction) -> Option<CallProgress> {
        match action {
            CallAction::PreAccept { .. } if !self.answered => Some(CallProgress::Ringing),
            CallAction::Accept { .. } if !self.answered => {
                self.answered = true;
                Some(CallProgress::Answered)
            }
            // `busy` is one of their devices that cannot take calls, while
            // the others ring on.
            CallAction::Reject { reason: None, .. } => {
                self.end.get_or_insert(CallEnd::Declined);
                None
            }
            CallAction::Terminate { .. } => {
                let end = if self.answered {
                    CallEnd::Finished
                } else {
                    CallEnd::Unanswered
                };
                self.end.get_or_insert(end);
                None
            }
            _ => None,
        }
    }

    fn fail(&mut self, why: String) {
        self.end.get_or_insert(CallEnd::Failed(why));
    }
}

/// `packet`, Opus wrapped the way the call's own codec carries it, as a
/// stock Opus decoder takes it.
fn standard_opus(packet: &[u8]) -> Option<Vec<u8>> {
    let mut packet = packet.to_vec();
    depacketize_opus_from_mlow(&mut packet).ok()?;
    Some(packet)
}

#[cfg(test)]
mod tests {
    use whatsapp_rust::Jid;
    use whatsapp_rust::chrono::Utc;
    use whatsapp_rust::voip::packetize_opus_for_mlow;

    use super::*;

    fn peer() -> Jid {
        Jid::pn("5511912345678")
    }

    fn signal(action: CallAction) -> Heard {
        Heard::Signal(Arc::new(Event::IncomingCall(IncomingCall::new_for_test(
            peer(),
            "stanza".into(),
            Utc::now(),
            action,
        ))))
    }

    fn accept(call: &str) -> Heard {
        signal(CallAction::Accept {
            call_id: call.into(),
            call_creator: peer(),
            audio: Vec::new(),
        })
    }

    fn reject(reason: Option<&str>) -> Heard {
        signal(CallAction::Reject {
            call_id: "CALL1".into(),
            call_creator: peer(),
            reason: reason.map(str::to_owned),
        })
    }

    fn terminate_event() -> Arc<Event> {
        Arc::new(Event::IncomingCall(IncomingCall::new_for_test(
            peer(),
            "stanza".into(),
            Utc::now(),
            CallAction::Terminate {
                call_id: "CALL1".into(),
                call_creator: peer(),
                reason: None,
                duration: None,
                audio_duration: None,
            },
        )))
    }

    fn terminate() -> Heard {
        Heard::Signal(terminate_event())
    }

    #[test]
    fn a_placed_call_is_answered_then_finishes() {
        let mut watch = Watch::new("CALL1", false);
        assert_eq!(watch.hear(&accept("CALL1")), Some(CallProgress::Answered));
        assert_eq!(
            watch.hear(&Heard::Media(CallEvent::RelayAllocated)),
            Some(CallProgress::Connected)
        );
        assert_eq!(watch.hear(&terminate()), None);
        assert_eq!(
            watch.hear(&Heard::Ended),
            Some(CallProgress::Ended(CallEnd::Finished))
        );
    }

    #[test]
    fn opus_from_the_other_end_is_unwrapped_for_a_stock_decoder() {
        // A 20 ms wideband CELT frame's table of contents, then its data:
        // the escape carries CELT only.
        let stock = vec![0xB8, 0x11, 0x22, 0x33, 0x44];
        let mut carried = stock.clone();
        assert!(packetize_opus_for_mlow(&mut carried).is_ok());
        assert_ne!(carried, stock);
        assert_eq!(standard_opus(&carried), Some(stock));
    }

    #[test]
    fn other_calls_signals_are_ignored() {
        let mut watch = Watch::new("CALL1", false);
        assert_eq!(watch.hear(&accept("CALL2")), None);
    }

    #[test]
    fn a_declined_call_says_so() {
        let mut watch = Watch::new("CALL1", false);
        // One of their devices being busy is not them declining.
        watch.hear(&reject(Some("busy")));
        watch.hear(&reject(None));
        watch.hear(&terminate());
        assert_eq!(
            watch.hear(&Heard::Ended),
            Some(CallProgress::Ended(CallEnd::Declined))
        );
    }

    #[test]
    fn a_call_ending_before_an_answer_went_unanswered() {
        let mut watch = Watch::new("CALL1", false);
        watch.hear(&terminate());
        assert_eq!(
            watch.hear(&Heard::Ended),
            Some(CallProgress::Ended(CallEnd::Unanswered))
        );
    }

    #[test]
    fn a_lost_connection_is_a_failure() {
        let mut watch = Watch::new("CALL1", true);
        watch.hear(&Heard::Media(CallEvent::RelayAllocateTimedOut));
        assert!(matches!(
            watch.hear(&Heard::Ended),
            Some(CallProgress::Ended(CallEnd::Failed(_)))
        ));
    }

    #[test]
    fn a_call_hung_up_elsewhere_stops_ringing() {
        let stopped = Rings.hear(&terminate_event());
        assert!(matches!(stopped, Some(RingChange::Stopped(id)) if id == "CALL1"));
    }
}
