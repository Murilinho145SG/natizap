//! Messages heard from WhatsApp, and whether each deserves a chime or a
//! notification given what is on screen.

use std::collections::HashSet;

use natizap_protocol::whatsapp_rust::prelude::Event;
use natizap_protocol::whatsapp_rust::types::events::InboundMessage;
use natizap_protocol::whatsapp_rust::wacore_binary::JidExt as _;
use natizap_protocol::{ChatId, Content};

use crate::{Preferences, Tone};

/// A message someone else sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Arrival {
    /// Every address its chat is known under.
    pub(crate) chat: Vec<ChatId>,
    pub(crate) group: bool,
    /// Every address its author is known under.
    pub(crate) author: Vec<ChatId>,
    /// The name the author chose for themselves.
    pub(crate) push_name: String,
    /// The message in one line: its text, or what kind it is.
    pub(crate) summary: String,
    /// Queued while this device was offline, rather than sent just now.
    pub(crate) offline: bool,
}

/// The messages in `event` that may deserve a chime: shown messages other
/// people sent to a chat. The account's own messages, from its other
/// devices, and status updates and channel posts, which the phone does not
/// notify about, are left out.
pub(crate) fn arrivals(event: &Event) -> Vec<Arrival> {
    event.messages().filter_map(arrival).collect()
}

fn arrival(inbound: &InboundMessage) -> Option<Arrival> {
    let source = &inbound.info.source;
    if source.is_from_me || source.chat.is_status_broadcast() || source.chat.is_newsletter() {
        return None;
    }
    let content = Content::of(&inbound.message)?;
    let id = |jid: &dyn ToString| ChatId::new(&jid.to_string());
    let author: Vec<ChatId> = std::iter::once(&source.sender)
        .chain(source.sender_alt.as_ref())
        .map(|jid| id(jid))
        .collect();
    let chat = if source.is_group {
        vec![id(&source.chat)]
    } else {
        std::iter::once(id(&source.chat))
            .chain(author.iter().cloned())
            .collect()
    };
    Some(Arrival {
        chat,
        group: source.is_group,
        author,
        push_name: inbound.info.push_name.clone(),
        summary: content.summary(),
        offline: inbound.info.is_offline,
    })
}

/// What is on screen and what is muted.
#[derive(Debug, Default)]
pub(crate) struct Quiet {
    pub(crate) focused: bool,
    pub(crate) open: HashSet<ChatId>,
    pub(crate) muted: HashSet<ChatId>,
}

impl Quiet {
    /// The tone `arrival` chimes with, if any: never in a muted chat or
    /// the chat the user is looking at, and in other chats while natizap
    /// is in use only when `preferences` say so.
    pub(crate) fn tone_for(&self, arrival: &Arrival, preferences: Preferences) -> Option<Tone> {
        let alert = if arrival.group {
            preferences.groups
        } else {
            preferences.messages
        };
        let silenced = !alert.sound
            || self.is_muted(arrival)
            || self.focused && (self.is_open(arrival) || !preferences.other_chats);
        (!silenced).then_some(alert.tone)
    }

    /// Whether `arrival` shows in the system's notifications: only while
    /// natizap is in the background, never for muted chats.
    pub(crate) fn shows(&self, arrival: &Arrival, preferences: Preferences) -> bool {
        preferences.popups && !self.focused && !self.is_muted(arrival)
    }

    fn is_muted(&self, arrival: &Arrival) -> bool {
        arrival.chat.iter().any(|id| self.muted.contains(id))
    }

    fn is_open(&self, arrival: &Arrival) -> bool {
        arrival.chat.iter().any(|id| self.open.contains(id))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::HashSet;

    use natizap_protocol::ChatId;

    use super::{Arrival, Quiet};
    use crate::{Alert, Preferences, Tone};

    pub(crate) const PERSON: &str = "5511900000001@s.whatsapp.net";
    const OTHER: &str = "5511900000002@s.whatsapp.net";
    pub(crate) const GROUP: &str = "120363000000000001@g.us";

    pub(crate) fn arrival(ids: &[&str]) -> Arrival {
        Arrival {
            chat: ids.iter().copied().map(ChatId::new).collect(),
            group: ids.iter().any(|id| id.ends_with("@g.us")),
            author: vec![ChatId::new(PERSON)],
            push_name: String::new(),
            summary: "Hi".into(),
            offline: false,
        }
    }

    fn quiet(focused: bool, open: &[&str], muted: &[&str]) -> Quiet {
        Quiet {
            focused,
            open: open.iter().copied().map(ChatId::new).collect(),
            muted: muted
                .iter()
                .copied()
                .map(ChatId::new)
                .collect::<HashSet<_>>(),
        }
    }

    fn chimes(quiet: &Quiet, ids: &[&str]) -> bool {
        quiet
            .tone_for(&arrival(ids), Preferences::default())
            .is_some()
    }

    #[test]
    fn stays_quiet_for_the_chat_on_screen() {
        assert!(!chimes(&quiet(true, &[PERSON], &[]), &[PERSON]));
    }

    #[test]
    fn chimes_for_another_chat() {
        assert!(chimes(&quiet(true, &[PERSON], &[]), &[OTHER]));
    }

    #[test]
    fn chimes_for_the_open_chat_when_the_window_is_not_in_use() {
        assert!(chimes(&quiet(false, &[PERSON], &[]), &[PERSON]));
    }

    #[test]
    fn knows_the_open_chat_by_its_other_address() {
        assert!(!chimes(
            &quiet(true, &[PERSON, "1234@lid"], &[]),
            &["1234@lid"]
        ));
    }

    #[test]
    fn never_chimes_for_a_muted_chat() {
        assert!(!chimes(&quiet(false, &[], &[GROUP]), &[GROUP]));
    }

    #[test]
    fn uses_the_groups_tone_for_groups() {
        let preferences = Preferences {
            groups: Alert {
                sound: true,
                tone: Tone::Marimba,
            },
            ..Preferences::default()
        };
        let tone = quiet(false, &[], &[]).tone_for(&arrival(&[GROUP]), preferences);
        assert_eq!(tone, Some(Tone::Marimba));
    }

    #[test]
    fn stays_quiet_for_groups_when_their_sound_is_off() {
        let preferences = Preferences {
            groups: Alert {
                sound: false,
                ..Alert::default()
            },
            ..Preferences::default()
        };
        let quiet = quiet(false, &[], &[]);
        assert_eq!(quiet.tone_for(&arrival(&[GROUP]), preferences), None);
        assert!(quiet.tone_for(&arrival(&[PERSON]), preferences).is_some());
    }

    #[test]
    fn keeps_other_chats_quiet_while_in_use_when_asked() {
        let preferences = Preferences {
            other_chats: false,
            ..Preferences::default()
        };
        assert_eq!(
            quiet(true, &[PERSON], &[]).tone_for(&arrival(&[OTHER]), preferences),
            None
        );
        assert!(
            quiet(false, &[PERSON], &[])
                .tone_for(&arrival(&[OTHER]), preferences)
                .is_some()
        );
    }

    #[test]
    fn shows_notifications_only_in_the_background() {
        let preferences = Preferences::default();
        assert!(quiet(false, &[PERSON], &[]).shows(&arrival(&[PERSON]), preferences));
        assert!(!quiet(true, &[], &[]).shows(&arrival(&[OTHER]), preferences));
    }

    #[test]
    fn shows_no_notifications_for_muted_chats_or_when_turned_off() {
        let off = Preferences {
            popups: false,
            ..Preferences::default()
        };
        assert!(!quiet(false, &[], &[GROUP]).shows(&arrival(&[GROUP]), Preferences::default()));
        assert!(!quiet(false, &[], &[]).shows(&arrival(&[PERSON]), off));
    }
}
