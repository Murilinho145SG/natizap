//! Which messages just arrived in the open chat, so they can animate in
//! while the rest stay still, even as the list scrolls.

use std::time::{Duration, Instant};

use gpui::SharedString;

/// How long a message counts as just arrived: long enough to animate in,
/// short enough that scrolling back to it later does not animate again.
const FRESH_FOR: Duration = Duration::from_secs(1);

/// The open chat's newest message and the messages that came after it.
#[derive(Debug, Default)]
pub(super) struct Arrivals {
    /// The newest message seen; `None` until the chat's messages show.
    newest: Option<SharedString>,
    fresh: Vec<(SharedString, Instant)>,
}

impl Arrivals {
    /// Starts over for a newly opened chat whose messages, oldest first,
    /// are `ids`: none of them arrived, they were already there.
    pub(super) fn reset<'a>(&mut self, ids: impl DoubleEndedIterator<Item = &'a SharedString>) {
        self.fresh.clear();
        self.newest = ids.last().cloned();
    }

    /// Notes the messages after the newest one seen in `ids`, oldest first,
    /// as arriving `now`. Older messages loaded in front of the list do
    /// not count, nor does a list that no longer holds the newest message.
    pub(super) fn saw<'a>(
        &mut self,
        ids: impl DoubleEndedIterator<Item = &'a SharedString>,
        now: Instant,
    ) {
        self.fresh
            .retain(|(_, arrived)| now.duration_since(*arrived) < FRESH_FOR);
        let mut after: Vec<SharedString> = Vec::new();
        let mut newest = None;
        let mut found = self.newest.is_none();
        for id in ids.rev() {
            newest.get_or_insert_with(|| id.clone());
            if Some(id) == self.newest.as_ref() {
                found = true;
                break;
            }
            after.push(id.clone());
        }
        // Without a newest message to start from, everything shown is
        // what the chat already held.
        if found && self.newest.is_some() {
            self.fresh.extend(after.into_iter().map(|id| (id, now)));
        }
        if newest.is_some() {
            self.newest = newest;
        }
    }

    /// Whether the message `id` arrived moments before `now`.
    pub(super) fn is_arriving(&self, id: &str, now: Instant) -> bool {
        self.fresh
            .iter()
            .any(|(fresh, arrived)| fresh == id && now.duration_since(*arrived) < FRESH_FOR)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use gpui::SharedString;

    use super::Arrivals;

    fn ids(names: &[&str]) -> Vec<SharedString> {
        names
            .iter()
            .map(|name| SharedString::from(name.to_string()))
            .collect()
    }

    #[test]
    fn counts_only_what_came_after_the_newest_message() {
        let now = Instant::now();
        let mut arrivals = Arrivals::default();
        arrivals.reset(ids(&["a", "b"]).iter());
        arrivals.saw(ids(&["a", "b", "c", "d"]).iter(), now);
        assert!(!arrivals.is_arriving("b", now));
        assert!(arrivals.is_arriving("c", now));
        assert!(arrivals.is_arriving("d", now));
    }

    #[test]
    fn ignores_older_messages_loaded_in_front() {
        let now = Instant::now();
        let mut arrivals = Arrivals::default();
        arrivals.reset(ids(&["c", "d"]).iter());
        arrivals.saw(ids(&["a", "b", "c", "d"]).iter(), now);
        assert!(!arrivals.is_arriving("a", now));
    }

    #[test]
    fn treats_the_first_messages_of_an_empty_chat_as_already_there() {
        let now = Instant::now();
        let mut arrivals = Arrivals::default();
        arrivals.reset(ids(&[]).iter());
        arrivals.saw(ids(&["a", "b"]).iter(), now);
        assert!(!arrivals.is_arriving("a", now));
        arrivals.saw(ids(&["a", "b", "c"]).iter(), now);
        assert!(arrivals.is_arriving("c", now));
    }

    #[test]
    fn stops_counting_a_message_as_arriving_soon_after() {
        let now = Instant::now();
        let mut arrivals = Arrivals::default();
        arrivals.reset(ids(&["a"]).iter());
        arrivals.saw(ids(&["a", "b"]).iter(), now);
        assert!(!arrivals.is_arriving("b", now + Duration::from_secs(2)));
    }
}
