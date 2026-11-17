//! Notifications: a chime when a message arrives that the user is not
//! looking at, as the phone sounds for messages outside the open chat.
//!
//! [`Notifier`] hears new messages itself; the binary tells it what is on
//! screen ([`Notifier::set_focused`], [`Notifier::set_open`]) and which
//! chats are muted ([`Notifier::set_muted`]).

mod chime;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{Context, Entity, Task};
use natizap_playback::Clip;
use natizap_protocol::whatsapp_rust::prelude::{Event, EventKind};
use natizap_protocol::whatsapp_rust::types::events::InboundMessage;
use natizap_protocol::whatsapp_rust::wacore_binary::JidExt as _;
use natizap_protocol::{ChatId, Content, Protocol};

/// The shortest gap between two chimes: a burst of messages, such as those
/// that queued up while offline, chimes once.
const QUIET_AFTER_CHIME: Duration = Duration::from_millis(1500);

/// Sounds a chime for messages that arrive where the user is not looking.
pub struct Notifier {
    quiet: Quiet,
    chimed: Option<Instant>,
    chime: Arc<[f32]>,
    _hearing: Task<()>,
}

impl std::fmt::Debug for Notifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Notifier")
            .field("quiet", &self.quiet)
            .finish_non_exhaustive()
    }
}

impl Notifier {
    /// Listens for messages arriving through `protocol`.
    pub fn new(protocol: &Entity<Protocol>, cx: &mut Context<Self>) -> Self {
        let events = protocol.read(cx).subscribe(&[EventKind::Messages]);
        let hearing = cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                let arrivals = arrivals(&event);
                if arrivals.is_empty() {
                    continue;
                }
                if this
                    .update(cx, |notifier, _| notifier.hear(&arrivals))
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            quiet: Quiet::default(),
            chimed: None,
            chime: chime::chime().into(),
            _hearing: hearing,
        }
    }

    /// Whether natizap's window is the one the user is using.
    pub fn set_focused(&mut self, focused: bool) {
        self.quiet.focused = focused;
    }

    /// The chat on screen, by every address it is known under; empty when
    /// none is.
    pub fn set_open(&mut self, addresses: HashSet<ChatId>) {
        self.quiet.open = addresses;
    }

    /// Every address of every muted chat.
    pub fn set_muted(&mut self, addresses: HashSet<ChatId>) {
        self.quiet.muted = addresses;
    }

    fn hear(&mut self, arrivals: &[Arrival]) {
        if !arrivals
            .iter()
            .any(|arrival| self.quiet.chimes_for(arrival))
        {
            return;
        }
        let now = Instant::now();
        if self
            .chimed
            .is_some_and(|chimed| now.duration_since(chimed) < QUIET_AFTER_CHIME)
        {
            return;
        }
        self.chimed = Some(now);
        let chime = Arc::clone(&self.chime);
        // A thread of its own: opening the sound card can take a moment,
        // and the card's stream must live until the chime has played.
        let playing = std::thread::Builder::new()
            .name("natizap-chime".into())
            .spawn(move || match Clip::play(&chime, chime::RATE) {
                Ok(clip) => std::thread::sleep(clip.length() + Duration::from_millis(150)),
                Err(error) => tracing::warn!(%error, "could not play the notification chime"),
            });
        if let Err(error) = playing {
            tracing::warn!(%error, "could not start the notification chime");
        }
    }
}

/// A message someone else sent, by every address its chat is known under.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Arrival {
    chat: Vec<ChatId>,
}

/// The messages in `event` that may deserve a chime: shown messages other
/// people sent to a chat. The account's own messages, from its other
/// devices, and status updates and channel posts, which the phone does not
/// chime for, are left out.
fn arrivals(event: &Event) -> Vec<Arrival> {
    event.messages().filter_map(arrival).collect()
}

fn arrival(inbound: &InboundMessage) -> Option<Arrival> {
    let source = &inbound.info.source;
    if source.is_from_me
        || source.chat.is_status_broadcast()
        || source.chat.is_newsletter()
        || Content::of(&inbound.message).is_none()
    {
        return None;
    }
    let chat = std::iter::once(&source.chat)
        .chain(source.sender_alt.as_ref().filter(|_| !source.is_group))
        .map(|jid| ChatId::new(&jid.to_string()))
        .collect();
    Some(Arrival { chat })
}

/// What keeps natizap from chiming.
#[derive(Debug, Default)]
struct Quiet {
    focused: bool,
    open: HashSet<ChatId>,
    muted: HashSet<ChatId>,
}

impl Quiet {
    /// Whether `arrival` chimes: never in a muted chat, and not in the
    /// chat the user is looking at.
    fn chimes_for(&self, arrival: &Arrival) -> bool {
        let any_of =
            |addresses: &HashSet<ChatId>| arrival.chat.iter().any(|id| addresses.contains(id));
        !any_of(&self.muted) && (!self.focused || !any_of(&self.open))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use natizap_protocol::ChatId;

    use super::{Arrival, Quiet};

    fn chat(id: &str) -> ChatId {
        ChatId::new(id)
    }

    fn arrival(ids: &[&str]) -> Arrival {
        Arrival {
            chat: ids.iter().copied().map(chat).collect(),
        }
    }

    fn quiet(focused: bool, open: &[&str], muted: &[&str]) -> Quiet {
        Quiet {
            focused,
            open: open.iter().copied().map(chat).collect(),
            muted: muted.iter().copied().map(chat).collect::<HashSet<_>>(),
        }
    }

    #[test]
    fn stays_quiet_for_the_chat_on_screen() {
        let quiet = quiet(true, &["5511900000001@s.whatsapp.net"], &[]);
        assert!(!quiet.chimes_for(&arrival(&["5511900000001@s.whatsapp.net"])));
    }

    #[test]
    fn chimes_for_another_chat() {
        let quiet = quiet(true, &["5511900000001@s.whatsapp.net"], &[]);
        assert!(quiet.chimes_for(&arrival(&["5511900000002@s.whatsapp.net"])));
    }

    #[test]
    fn chimes_for_the_open_chat_when_the_window_is_not_in_use() {
        let quiet = quiet(false, &["5511900000001@s.whatsapp.net"], &[]);
        assert!(quiet.chimes_for(&arrival(&["5511900000001@s.whatsapp.net"])));
    }

    #[test]
    fn knows_the_open_chat_by_its_other_address() {
        let quiet = quiet(true, &["5511900000001@s.whatsapp.net", "1234@lid"], &[]);
        assert!(!quiet.chimes_for(&arrival(&["1234@lid"])));
    }

    #[test]
    fn never_chimes_for_a_muted_chat() {
        let quiet = quiet(false, &[], &["120363000000000001@g.us"]);
        assert!(!quiet.chimes_for(&arrival(&["120363000000000001@g.us"])));
    }
}
