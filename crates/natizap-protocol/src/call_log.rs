//! The account's call history: every call that rang on its devices or was
//! made from its phone, how it ended and how long it lasted.

use std::collections::HashMap;

use whatsapp_rust::prelude::{Event, EventKind, wa};
use whatsapp_rust::wacore::types::call::{CallAction, ElsewhereOutcome};

use crate::ChatId;
use crate::live_call::AnsweredHere;

/// How a call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallOutcome {
    /// Someone picked up, on this device or, for a call made, wherever.
    Answered,
    /// A call that came in was picked up on another of the account's
    /// devices.
    AnsweredElsewhere,
    /// It rang and nobody picked up: a missed call when it came in, an
    /// unanswered one when the account made it.
    Missed,
    /// It was turned down.
    Declined,
}

impl CallOutcome {
    /// How the call history file spells it.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Answered => "answered",
            Self::AnsweredElsewhere => "answered_elsewhere",
            Self::Missed => "missed",
            Self::Declined => "declined",
        }
    }

    /// The outcome spelled `key`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        [
            Self::Answered,
            Self::AnsweredElsewhere,
            Self::Missed,
            Self::Declined,
        ]
        .into_iter()
        .find(|outcome| outcome.key() == key)
    }
}

/// One call in the history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallRecord {
    /// WhatsApp's id for the call.
    pub id: String,
    /// Who was on the other end: the caller, or who the account called.
    pub peer: ChatId,
    /// The group, for a group call.
    pub group: Option<ChatId>,
    /// It came in, rather than being made by the account.
    pub incoming: bool,
    pub video: bool,
    pub outcome: CallOutcome,
    /// When it started ringing, in Unix seconds.
    pub at: i64,
    /// How long it lasted, when answered.
    pub seconds: Option<u32>,
}

/// A call still ringing.
#[derive(Debug, Clone)]
struct Ringing {
    peer: ChatId,
    group: Option<ChatId>,
    video: bool,
    at: i64,
    outcome: Option<CallOutcome>,
}

/// Follows calls through their events into [`CallRecord`]s. Feed it every
/// event of [`CallFollower::EVENTS`]. Made by
/// [`Protocol::call_follower`](crate::Protocol::call_follower).
#[derive(Debug)]
pub struct CallFollower {
    ringing: HashMap<String, Ringing>,
    answered: AnsweredHere,
}

impl CallFollower {
    pub(crate) fn new(answered: AnsweredHere) -> Self {
        Self {
            ringing: HashMap::new(),
            answered,
        }
    }

    /// The events calls are followed through.
    pub const EVENTS: &[EventKind] = &[
        EventKind::IncomingCall,
        EventKind::MissedCall,
        EventKind::CallEndedElsewhere,
        EventKind::HistorySync,
    ];

    /// The calls `event` finished telling about.
    pub fn hear(&mut self, event: &Event) -> Vec<CallRecord> {
        match event {
            Event::IncomingCall(call) => {
                let at = crate::calls::sent_at(call);
                let peer = ChatId::new(&call.from.to_non_ad().to_string());
                self.action(&call.action, peer, at).into_iter().collect()
            }
            Event::CallEndedElsewhere(ended) => {
                if let Some(ringing) = self.ringing.get_mut(&ended.call_id) {
                    ringing.outcome = match ended.outcome {
                        ElsewhereOutcome::Accepted => Some(CallOutcome::AnsweredElsewhere),
                        ElsewhereOutcome::Rejected => Some(CallOutcome::Declined),
                        _ => ringing.outcome,
                    };
                }
                Vec::new()
            }
            // A call that rang while natizap was offline, replayed without
            // its offer: whether it was a video call is lost.
            Event::MissedCall(missed) => vec![CallRecord {
                id: missed.call_id.clone(),
                peer: ChatId::new(&missed.from.to_non_ad().to_string()),
                group: None,
                incoming: true,
                video: false,
                outcome: CallOutcome::Missed,
                at: missed.timestamp.as_secs(),
                seconds: None,
            }],
            Event::HistorySync(history) => history.get().map_or_else(Vec::new, |history| {
                history
                    .call_log_records
                    .iter()
                    .filter_map(from_history)
                    .collect()
            }),
            _ => Vec::new(),
        }
    }

    fn action(&mut self, action: &CallAction, peer: ChatId, at: i64) -> Option<CallRecord> {
        match action {
            CallAction::Offer {
                call_id,
                is_video,
                group_jid,
                ..
            } => {
                self.ringing.entry(call_id.clone()).or_insert(Ringing {
                    peer,
                    group: group_jid
                        .as_ref()
                        .map(|group| ChatId::new(&group.to_string())),
                    video: *is_video,
                    at,
                    outcome: None,
                });
                None
            }
            CallAction::OfferNotice {
                call_id, is_video, ..
            } => {
                self.ringing.entry(call_id.clone()).or_insert(Ringing {
                    peer,
                    group: None,
                    video: *is_video,
                    at,
                    outcome: None,
                });
                None
            }
            // Heard here, an incoming call's answer came from another of
            // the account's devices: this one sends its own.
            CallAction::Accept { call_id, .. } => {
                if let Some(ringing) = self.ringing.get_mut(call_id) {
                    ringing.outcome = Some(CallOutcome::AnsweredElsewhere);
                }
                None
            }
            // `busy` is one device that cannot take the call, while the
            // others keep ringing; no reason is the user declining.
            CallAction::Reject {
                call_id,
                reason: None,
                ..
            } => {
                if let Some(ringing) = self.ringing.get_mut(call_id) {
                    ringing.outcome = Some(CallOutcome::Declined);
                }
                None
            }
            CallAction::Terminate {
                call_id,
                reason,
                duration,
                ..
            } => {
                let ringing = self.ringing.remove(call_id)?;
                let answered_here = self
                    .answered
                    .contains(call_id)
                    .then_some(CallOutcome::Answered);
                let outcome = match (answered_here.or(ringing.outcome), reason.as_deref()) {
                    (Some(outcome), _) => outcome,
                    (None, Some("accepted_elsewhere")) => CallOutcome::AnsweredElsewhere,
                    (None, Some("rejected_elsewhere")) => CallOutcome::Declined,
                    (None, _) => CallOutcome::Missed,
                };
                Some(CallRecord {
                    id: call_id.clone(),
                    peer: ringing.peer,
                    group: ringing.group,
                    incoming: true,
                    video: ringing.video,
                    outcome,
                    at: ringing.at,
                    seconds: duration.filter(|_| {
                        matches!(
                            outcome,
                            CallOutcome::Answered | CallOutcome::AnsweredElsewhere
                        )
                    }),
                })
            }
            _ => None,
        }
    }
}

/// A call from the history the phone sends right after linking.
fn from_history(record: &wa::CallLogRecord) -> Option<CallRecord> {
    use wa::call_log_record::CallResult;
    let incoming = record.is_incoming.unwrap_or(false);
    let group = record.group_jid.as_deref().map(ChatId::new);
    let peer = if incoming {
        record.call_creator_jid.as_deref().map(ChatId::new)
    } else {
        record
            .participants
            .iter()
            .find_map(|participant| participant.user_jid.as_deref())
            .map(ChatId::new)
    }
    .or_else(|| group.clone())?;
    let outcome = match record.call_result? {
        CallResult::CONNECTED => CallOutcome::Answered,
        CallResult::ACCEPTEDELSEWHERE => CallOutcome::AnsweredElsewhere,
        CallResult::REJECTED => CallOutcome::Declined,
        CallResult::MISSED
        | CallResult::CANCELLED
        | CallResult::UNAVAILABLE
        | CallResult::ABANDONED
        | CallResult::FAILED => CallOutcome::Missed,
        // Not over yet, or a result added later: nothing to file.
        _ => return None,
    };
    Some(CallRecord {
        id: record.call_id.clone()?,
        peer,
        group,
        incoming,
        video: record.is_video.unwrap_or(false),
        outcome,
        at: record.start_time?,
        seconds: record
            .duration
            .and_then(|seconds| u32::try_from(seconds).ok())
            .filter(|seconds| *seconds > 0),
    })
}

#[cfg(test)]
mod tests {
    use whatsapp_rust::Jid;
    use whatsapp_rust::wacore::types::call::IncomingCall;

    use super::*;

    fn caller() -> Jid {
        Jid::pn("5511912345678")
    }

    fn stanza(action: CallAction) -> Event {
        Event::IncomingCall(Box::new(
            IncomingCall::builder()
                .from(caller())
                .stanza_id("stanza".into())
                .timestamp(whatsapp_rust::time::now_utc())
                .offline(false)
                .action(action)
                .build(),
        ))
    }

    fn offer() -> Event {
        stanza(CallAction::Offer {
            call_id: "CALL1".into(),
            call_creator: caller(),
            caller_pn: None,
            caller_country_code: None,
            device_class: None,
            joinable: false,
            is_video: true,
            audio: Vec::new(),
            group_jid: None,
        })
    }

    fn terminate(reason: Option<&str>, duration: Option<u32>) -> Event {
        stanza(CallAction::Terminate {
            call_id: "CALL1".into(),
            call_creator: caller(),
            reason: reason.map(str::to_owned),
            duration,
            audio_duration: None,
        })
    }

    fn ended(follower: &mut CallFollower, last: &Event) -> Option<(CallOutcome, Option<u32>)> {
        follower
            .hear(last)
            .first()
            .map(|record| (record.outcome, record.seconds))
    }

    #[test]
    fn a_call_answered_on_the_phone_keeps_its_length() {
        let mut follower = CallFollower::new(AnsweredHere::default());
        assert_eq!(follower.hear(&offer()), Vec::new());
        assert_eq!(
            ended(
                &mut follower,
                &terminate(Some("accepted_elsewhere"), Some(73))
            ),
            Some((CallOutcome::AnsweredElsewhere, Some(73)))
        );
    }

    #[test]
    fn a_call_answered_here_keeps_its_length() {
        let answered = AnsweredHere::default();
        let mut follower = CallFollower::new(answered.clone());
        follower.hear(&offer());
        answered.insert("CALL1");
        assert_eq!(
            ended(&mut follower, &terminate(None, Some(42))),
            Some((CallOutcome::Answered, Some(42)))
        );
    }

    #[test]
    fn a_call_nobody_picked_up_is_missed_without_a_length() {
        let mut follower = CallFollower::new(AnsweredHere::default());
        follower.hear(&offer());
        assert_eq!(
            ended(&mut follower, &terminate(Some("timeout"), Some(30))),
            Some((CallOutcome::Missed, None))
        );
        // Reported once.
        assert_eq!(follower.hear(&terminate(None, None)), Vec::new());
    }

    #[test]
    fn a_declined_call_is_declined() {
        let mut follower = CallFollower::new(AnsweredHere::default());
        follower.hear(&offer());
        follower.hear(&stanza(CallAction::Reject {
            call_id: "CALL1".into(),
            call_creator: caller(),
            reason: None,
        }));
        assert_eq!(
            ended(&mut follower, &terminate(None, None)),
            Some((CallOutcome::Declined, None))
        );
    }

    #[test]
    fn spells_outcomes_both_ways() {
        for outcome in [
            CallOutcome::Answered,
            CallOutcome::Missed,
            CallOutcome::Declined,
        ] {
            assert_eq!(CallOutcome::from_key(outcome.key()), Some(outcome));
        }
    }
}
