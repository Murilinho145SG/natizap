//! Calls natizap hears ring but cannot answer yet: which of them nobody
//! picked up, so their chat can say so.

use std::collections::HashMap;

use whatsapp_rust::prelude::{Event, EventKind};
use whatsapp_rust::wacore::types::call::CallAction;

use crate::ChatId;
use crate::live_call::AnsweredHere;

/// A call that rang on the account's devices and that nobody answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissedCall {
    /// WhatsApp's id for the call; the same call is reported once.
    pub id: String,
    /// The chat it belongs to: the group for a group call, else the caller.
    pub chat: ChatId,
    /// Who called, in a group call.
    pub caller: Option<ChatId>,
    /// It was a video call.
    pub video: bool,
    /// When it rang, in Unix seconds.
    pub at: i64,
}

/// What is known about a call still ringing.
#[derive(Debug, Clone)]
struct Ring {
    chat: ChatId,
    caller: Option<ChatId>,
    video: bool,
}

/// Follows the calls ringing on the account, telling which end missed.
/// Feed it every event of [`Ringing::EVENTS`].
/// Made by [`Protocol::ringing`](crate::Protocol::ringing).
#[derive(Debug)]
pub struct Ringing {
    rings: HashMap<String, Ring>,
    answered: AnsweredHere,
}

impl Ringing {
    pub(crate) fn new(answered: AnsweredHere) -> Self {
        Self {
            rings: HashMap::new(),
            answered,
        }
    }

    /// The events a call is followed through.
    pub const EVENTS: &[EventKind] = &[EventKind::IncomingCall, EventKind::MissedCall];

    /// The call `event` says was missed, if it says so.
    pub fn hear(&mut self, event: &Event) -> Option<MissedCall> {
        match event {
            Event::IncomingCall(call) => {
                let caller = ChatId::new(&call.from.to_non_ad().to_string());
                match &call.action {
                    CallAction::Offer {
                        call_id,
                        is_video,
                        group_jid,
                        ..
                    } => {
                        let ring = match group_jid {
                            Some(group) => Ring {
                                chat: ChatId::new(&group.to_string()),
                                caller: Some(caller),
                                video: *is_video,
                            },
                            None => Ring {
                                chat: caller,
                                caller: None,
                                video: *is_video,
                            },
                        };
                        self.rings.insert(call_id.clone(), ring);
                        None
                    }
                    CallAction::OfferNotice {
                        call_id, is_video, ..
                    } => {
                        self.rings.entry(call_id.clone()).or_insert(Ring {
                            chat: caller,
                            caller: None,
                            video: *is_video,
                        });
                        None
                    }
                    CallAction::Accept { call_id, .. } | CallAction::Reject { call_id, .. } => {
                        self.rings.remove(call_id);
                        None
                    }
                    CallAction::Terminate {
                        call_id, reason, ..
                    } => {
                        let ring = self.rings.remove(call_id)?;
                        if self.answered.contains(call_id) {
                            return None;
                        }
                        // How WhatsApp Web files a call that ended before
                        // anyone answered; `accepted_elsewhere` and
                        // `rejected_elsewhere` mean another device did.
                        matches!(
                            reason.as_deref(),
                            None | Some("timeout" | "group_call_ended")
                        )
                        .then(|| MissedCall {
                            id: call_id.clone(),
                            chat: ring.chat,
                            caller: ring.caller,
                            video: ring.video,
                            at: call.timestamp.timestamp(),
                        })
                    }
                    _ => None,
                }
            }
            // A call that rang while natizap was offline, replayed on
            // connecting. The client reports it without its offer, so
            // whether it was a video call is lost: it reads as a voice call.
            Event::MissedCall(missed) => Some(MissedCall {
                id: missed.call_id.clone(),
                chat: ChatId::new(&missed.from.to_non_ad().to_string()),
                caller: None,
                video: false,
                at: missed.timestamp.timestamp(),
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use whatsapp_rust::Jid;
    use whatsapp_rust::chrono::Utc;
    use whatsapp_rust::wacore::types::call::IncomingCall;

    use super::*;

    fn caller() -> Jid {
        Jid::pn("5511912345678")
    }

    fn stanza(action: CallAction) -> Event {
        Event::IncomingCall(IncomingCall::new_for_test(
            caller(),
            "stanza".into(),
            Utc::now(),
            action,
        ))
    }

    fn offer(video: bool) -> Event {
        stanza(CallAction::Offer {
            call_id: "CALL1".into(),
            call_creator: caller(),
            caller_pn: None,
            caller_country_code: None,
            device_class: None,
            joinable: false,
            is_video: video,
            audio: Vec::new(),
            group_jid: None,
        })
    }

    fn terminate(reason: Option<&str>) -> Event {
        stanza(CallAction::Terminate {
            call_id: "CALL1".into(),
            call_creator: caller(),
            reason: reason.map(str::to_owned),
            duration: None,
            audio_duration: None,
        })
    }

    #[test]
    fn a_call_that_stops_ringing_unanswered_is_missed() {
        let mut ringing = Ringing::new(AnsweredHere::default());
        assert_eq!(ringing.hear(&offer(true)), None);
        let missed = ringing.hear(&terminate(Some("timeout")));
        assert_eq!(
            missed
                .as_ref()
                .map(|missed| (missed.id.as_str(), &missed.chat, missed.video)),
            Some(("CALL1", &ChatId::new("5511912345678@s.whatsapp.net"), true))
        );
        // The same call ending twice is reported once.
        assert_eq!(ringing.hear(&terminate(None)), None);
    }

    #[test]
    fn a_call_answered_elsewhere_is_not_missed() {
        let mut ringing = Ringing::new(AnsweredHere::default());
        ringing.hear(&offer(false));
        assert_eq!(ringing.hear(&terminate(Some("accepted_elsewhere"))), None);
    }

    #[test]
    fn a_call_answered_here_is_not_missed() {
        let answered = AnsweredHere::default();
        let mut ringing = Ringing::new(answered.clone());
        ringing.hear(&offer(false));
        answered.insert("CALL1");
        assert_eq!(ringing.hear(&terminate(None)), None);
    }

    #[test]
    fn a_call_ending_without_ringing_here_is_not_missed() {
        // natizap's own outgoing calls, or ones it joined after the offer.
        assert_eq!(
            Ringing::new(AnsweredHere::default()).hear(&terminate(None)),
            None
        );
    }
}
