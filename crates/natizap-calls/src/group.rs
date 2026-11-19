//! Calls shown together: back-to-back calls with the same person that went
//! the same way read as one row with a count, as on the phone.

use natizap_protocol::{CallOutcome, CallRecord};

/// The longest gap between two calls that still show as one row.
const WITHIN: i64 = 60 * 60;

/// Calls that show as one row, newest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallGroup {
    pub calls: Vec<CallRecord>,
}

impl CallGroup {
    /// The newest call, which the row shows.
    #[must_use]
    pub fn latest(&self) -> &CallRecord {
        &self.calls[0]
    }
}

/// Whether `next`, older, joins the row `call` is in.
fn joins(call: &CallRecord, next: &CallRecord) -> bool {
    let missed = |record: &CallRecord| record.outcome == CallOutcome::Missed;
    call.peer == next.peer
        && call.group == next.group
        && call.incoming == next.incoming
        && missed(call) == missed(next)
        && call.at - next.at <= WITHIN
}

/// `calls`, newest first, gathered into rows.
pub(crate) fn collapse(calls: &[CallRecord]) -> Vec<CallGroup> {
    let mut groups: Vec<CallGroup> = Vec::new();
    for call in calls {
        match groups.last_mut() {
            Some(group) if group.calls.last().is_some_and(|last| joins(last, call)) => {
                group.calls.push(call.clone());
            }
            _ => groups.push(CallGroup {
                calls: vec![call.clone()],
            }),
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use natizap_protocol::ChatId;

    use super::*;

    fn call(id: &str, peer: &str, at: i64, outcome: CallOutcome) -> CallRecord {
        CallRecord {
            id: id.into(),
            peer: ChatId::new(peer),
            group: None,
            incoming: true,
            video: false,
            outcome,
            at,
            seconds: None,
        }
    }

    fn rows(groups: &[CallGroup]) -> Vec<Vec<&str>> {
        groups
            .iter()
            .map(|group| group.calls.iter().map(|call| call.id.as_str()).collect())
            .collect()
    }

    #[test]
    fn gathers_back_to_back_missed_calls_from_one_person() {
        let calls = [
            call("3", "a@lid", 10_000, CallOutcome::Missed),
            call("2", "a@lid", 9_000, CallOutcome::Missed),
            call("1", "a@lid", 8_000, CallOutcome::Missed),
        ];
        assert_eq!(rows(&collapse(&calls)), [vec!["3", "2", "1"]]);
    }

    #[test]
    fn keeps_apart_other_people_answered_calls_and_long_gaps() {
        let calls = [
            call("5", "a@lid", 100_000, CallOutcome::Missed),
            call("4", "a@lid", 99_000, CallOutcome::Answered),
            call("3", "b@lid", 98_000, CallOutcome::Answered),
            call("2", "a@lid", 97_000, CallOutcome::Answered),
            call("1", "a@lid", 97_000 - WITHIN - 1, CallOutcome::Answered),
        ];
        assert_eq!(
            rows(&collapse(&calls)),
            [vec!["5"], vec!["4"], vec!["3"], vec!["2"], vec!["1"]]
        );
    }

    #[test]
    fn answered_and_declined_calls_share_a_row() {
        let calls = [
            call("2", "a@lid", 2_000, CallOutcome::Declined),
            call("1", "a@lid", 1_000, CallOutcome::Answered),
        ];
        assert_eq!(rows(&collapse(&calls)), [vec!["2", "1"]]);
    }
}
