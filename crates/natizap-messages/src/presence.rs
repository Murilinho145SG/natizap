//! Whether the person in the open chat is online, typing or recording, and
//! when they were last seen, as the header says it.

use chrono::{DateTime, Datelike as _, Local, TimeZone as _};
use natizap_protocol::ChatId;
use natizap_protocol::whatsapp_rust::prelude::Event;
use natizap_protocol::whatsapp_rust::types::presence::{ChatPresence, ChatPresenceMedia};

/// What the person in a one-to-one chat is doing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Presence {
    pub(crate) online: bool,
    /// When they were last online, in Unix seconds, if they share it.
    pub(crate) last_seen: Option<i64>,
    pub(crate) activity: Option<Activity>,
}

/// What someone is doing in the chat right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Activity {
    Typing,
    Recording,
}

impl Presence {
    /// Applies `event` when it is about someone at `addresses`.
    pub(crate) fn follow(&mut self, event: &Event, addresses: &[ChatId]) -> bool {
        let about = |jid: &natizap_protocol::whatsapp_rust::Jid| {
            addresses.contains(&ChatId::new(&jid.to_non_ad().to_string()))
        };
        match event {
            Event::Presence(update) if about(&update.from) => {
                self.online = !update.unavailable;
                if let Some(last_seen) = update.last_seen {
                    self.last_seen = Some(last_seen.timestamp());
                }
                if !self.online {
                    self.activity = None;
                }
                true
            }
            Event::ChatPresence(update) if about(&update.source.chat) => {
                self.activity = match (update.state, update.media) {
                    (ChatPresence::Composing, ChatPresenceMedia::Audio) => {
                        Some(Activity::Recording)
                    }
                    (ChatPresence::Composing, ChatPresenceMedia::Text) => Some(Activity::Typing),
                    (ChatPresence::Paused, _) => None,
                };
                // Typing shows they are here.
                self.online |= self.activity.is_some();
                true
            }
            _ => false,
        }
    }

    /// The header's line: "typing…", "online", or when they were last seen.
    pub(crate) fn label(&self, now: DateTime<Local>) -> Option<String> {
        match self.activity {
            Some(Activity::Typing) => return Some("typing…".to_owned()),
            Some(Activity::Recording) => return Some("recording audio…".to_owned()),
            None => {}
        }
        if self.online {
            return Some("online".to_owned());
        }
        let seen = Local.timestamp_opt(self.last_seen?, 0).single()?;
        let days_ago = (now.date_naive() - seen.date_naive()).num_days();
        let time = seen.format("%H:%M");
        Some(match days_ago {
            ..=0 => format!("last seen today at {time}"),
            1 => format!("last seen yesterday at {time}"),
            _ if seen.year() == now.year() => {
                format!("last seen {} at {time}", seen.format("%d/%m"))
            }
            _ => format!("last seen {} at {time}", seen.format("%d/%m/%Y")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn says_what_the_person_is_doing_or_when_they_were_here() {
        let Some(now) = Local.with_ymd_and_hms(2026, 11, 17, 15, 0, 0).single() else {
            panic!("test date must exist in the local zone");
        };
        let at = |day, hour| {
            Local
                .with_ymd_and_hms(2026, 11, day, hour, 22, 0)
                .single()
                .map(|at| at.timestamp())
        };
        let mut seen = Presence {
            last_seen: at(17, 5),
            ..Presence::default()
        };
        assert_eq!(seen.label(now).as_deref(), Some("last seen today at 05:22"));
        seen.last_seen = at(16, 22);
        assert_eq!(
            seen.label(now).as_deref(),
            Some("last seen yesterday at 22:22")
        );
        seen.online = true;
        assert_eq!(seen.label(now).as_deref(), Some("online"));
        seen.activity = Some(Activity::Typing);
        assert_eq!(seen.label(now).as_deref(), Some("typing…"));
        assert_eq!(Presence::default().label(now), None);
    }
}
