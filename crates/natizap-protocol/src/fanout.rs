use std::sync::{Arc, Mutex, PoisonError};

use whatsapp_rust::prelude::{Event, EventHandler, EventInterest};

/// Forwards each protocol event to every subscriber interested in its kind.
///
/// Clones share one subscriber list: the [`crate::Protocol`] keeps one and
/// hands a clone to each client it starts, so subscriptions survive client
/// restarts.
#[derive(Debug, Default, Clone)]
pub(crate) struct Fanout {
    subscribers: Arc<Mutex<Vec<Subscriber>>>,
}

#[derive(Debug)]
struct Subscriber {
    interest: EventInterest,
    tx: async_channel::Sender<Arc<Event>>,
}

impl Fanout {
    pub(crate) fn subscribe(&self, interest: EventInterest) -> async_channel::Receiver<Arc<Event>> {
        let (tx, rx) = async_channel::unbounded();
        self.lock().push(Subscriber { interest, tx });
        rx
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Subscriber>> {
        // A panic while holding the lock cannot leave the list half-updated,
        // so a poisoned lock is still safe to use.
        self.subscribers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

impl EventHandler for Fanout {
    fn handle_event(&self, event: Arc<Event>) {
        let kind = event.kind();
        self.lock().retain(|subscriber| {
            if !subscriber.interest.wants(kind) {
                return true;
            }
            // A closed channel means the subscriber was dropped; forget it.
            subscriber.tx.try_send(event.clone()).is_ok()
        });
    }
}

#[cfg(test)]
mod tests {
    use whatsapp_rust::prelude::EventKind;
    use whatsapp_rust::types::events::StreamReplaced;

    use super::*;

    fn replaced() -> Arc<Event> {
        Arc::new(Event::StreamReplaced(StreamReplaced::builder().build()))
    }

    #[test]
    fn delivers_only_the_kinds_each_subscriber_asked_for() {
        let fanout = Fanout::default();
        let wants = fanout.subscribe(EventInterest::of(&[EventKind::StreamReplaced]));
        let ignores = fanout.subscribe(EventInterest::of(&[EventKind::Connected]));

        fanout.handle_event(replaced());

        assert!(matches!(
            wants.try_recv().as_deref(),
            Ok(Event::StreamReplaced(_))
        ));
        assert!(ignores.try_recv().is_err());
    }

    #[test]
    fn keeps_delivering_after_a_subscriber_goes_away() {
        let fanout = Fanout::default();
        drop(fanout.subscribe(EventInterest::ALL));
        let alive = fanout.subscribe(EventInterest::ALL);

        fanout.handle_event(replaced());
        fanout.handle_event(replaced());

        assert_eq!(alive.len(), 2);
    }
}
