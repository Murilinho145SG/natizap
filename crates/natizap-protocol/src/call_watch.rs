//! Following one live call: its signals, its media's events and its end,
//! told as [`CallProgress`]; Opus the other end sends goes on to be
//! decoded.

use std::sync::Arc;

use futures::StreamExt as _;
use whatsapp_rust::prelude::Event;
use whatsapp_rust::voip::{CallEvent, CallHandle, depacketize_opus_from_mlow};
use whatsapp_rust::wacore::types::call::CallAction;

use crate::live_call::{CallEnd, CallProgress};

/// Something a call's watch heard.
enum Heard {
    Signal(Arc<Event>),
    Media(CallEvent),
    Ended,
}

/// Follows one call into [`CallProgress`].
pub(crate) struct Watch {
    id: String,
    answered: bool,
    /// How it ends, once something said so.
    end: Option<CallEnd>,
}

impl Watch {
    pub(crate) fn new(id: &str, answered: bool) -> Self {
        Self {
            id: id.to_owned(),
            answered,
            end: None,
        }
    }

    pub(crate) async fn run(
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
        let (mut rtcp, mut rtcp_audio, mut reported) = (0_u64, 0_u64, 0_u64);
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
                Heard::Media(CallEvent::RtcpReceived {
                    reports_audio,
                    report_blocks,
                    ..
                }) => {
                    rtcp += 1;
                    rtcp_audio += u64::from(*reports_audio);
                    reported += u64::try_from(report_blocks.len()).unwrap_or(0);
                }
                Heard::Signal(_) | Heard::Ended => {}
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
                    rtcp,
                    rtcp_audio,
                    reported,
                    "call ended: Opus packets from the other end and how many unwrapped, \
                     RTCP packets heard, ones about audio, report blocks"
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
    use whatsapp_rust::wacore::types::call::IncomingCall;

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
}
