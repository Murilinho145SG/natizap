//! What a system notification says about the messages that arrived.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{SharedString, SystemNotification};
use natizap_protocol::{ChatId, PersonName};

use crate::heard::Arrival;

/// The tag of the notification about messages queued while offline, which
/// opens no chat in particular.
pub(crate) const AWAY_TAG: &str = "natizap:away";

/// The names notifications show, as the chat list knows them.
#[derive(Debug, Default)]
pub(crate) struct Directory {
    /// Each chat's title, by every address it is known under.
    pub(crate) titles: HashMap<ChatId, SharedString>,
    /// Everyone's names, by every address they are known under.
    pub(crate) names: Arc<HashMap<ChatId, PersonName>>,
}

impl Directory {
    /// The notifications for `arrivals`, one per chat, in the order the
    /// chats first appear; each shows the chat's newest message, or only
    /// that there is one when `previews` is off.
    pub(crate) fn popups(&self, arrivals: &[&Arrival], previews: bool) -> Vec<SystemNotification> {
        let mut chats: Vec<(&Arrival, usize)> = Vec::new();
        for arrival in arrivals {
            match chats
                .iter_mut()
                .find(|(seen, _)| seen.chat.first() == arrival.chat.first())
            {
                Some((newest, count)) => {
                    *newest = arrival;
                    *count += 1;
                }
                None => chats.push((arrival, 1)),
            }
        }
        chats
            .into_iter()
            .map(|(arrival, count)| self.popup(arrival, count, previews))
            .collect()
    }

    fn popup(&self, arrival: &Arrival, count: usize, previews: bool) -> SystemNotification {
        let chat = self.chat_title(arrival);
        let title = if count > 1 {
            format!("{chat} · {count} messages")
        } else {
            chat
        };
        let body = if !previews {
            "New message".to_owned()
        } else if arrival.group {
            format!("{}: {}", self.author_name(arrival), arrival.summary)
        } else {
            arrival.summary.clone()
        };
        SystemNotification {
            tag: arrival
                .chat
                .first()
                .map_or_else(SharedString::default, |id| id.as_str().to_owned().into()),
            title: escaped(&title).into(),
            body: escaped(&body).into(),
            actions: Vec::new(),
        }
    }

    fn chat_title(&self, arrival: &Arrival) -> String {
        arrival
            .chat
            .iter()
            .find_map(|id| self.titles.get(id))
            .map_or_else(|| self.author_name(arrival), ToString::to_string)
    }

    /// The author's name as the phone shows it: as saved in the user's
    /// contacts, else as they chose it, else their number.
    fn author_name(&self, arrival: &Arrival) -> String {
        let known = arrival.author.iter().find_map(|id| self.names.get(id));
        known
            .and_then(|name| name.saved.clone().or_else(|| name.chosen.clone()))
            .or_else(|| (!arrival.push_name.is_empty()).then(|| arrival.push_name.clone()))
            .or_else(|| arrival.author.iter().find_map(ChatId::phone_number))
            .unwrap_or_else(|| "Someone".to_owned())
    }
}

/// The notification about messages that queued up while natizap was
/// offline: one for all of them, instead of a flood.
pub(crate) fn away() -> SystemNotification {
    SystemNotification {
        tag: AWAY_TAG.into(),
        title: "natizap".into(),
        body: "New messages arrived while you were offline".into(),
        actions: Vec::new(),
    }
}

/// `text` safe from notification servers that read markup, such as GNOME
/// Shell's, which would otherwise swallow anything that looks like a tag.
fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use natizap_protocol::{ChatId, PersonName};

    use super::Directory;
    use crate::heard::tests::{GROUP, PERSON, arrival};

    fn directory() -> Directory {
        Directory {
            titles: HashMap::from([
                (ChatId::new(PERSON), "Ana Souza".into()),
                (ChatId::new(GROUP), "Clube do Livro".into()),
            ]),
            names: Arc::new(HashMap::from([(
                ChatId::new(PERSON),
                PersonName {
                    saved: Some("Ana".into()),
                    chosen: Some("Aninha".into()),
                },
            )])),
        }
    }

    #[test]
    fn names_the_chat_and_shows_the_message() {
        let popups = directory().popups(&[&arrival(&[PERSON])], true);
        assert_eq!(popups.len(), 1);
        assert_eq!(popups[0].title.as_ref(), "Ana Souza");
        assert_eq!(popups[0].body.as_ref(), "Hi");
        assert_eq!(popups[0].tag.as_ref(), PERSON);
    }

    #[test]
    fn says_who_wrote_in_a_group() {
        let popups = directory().popups(&[&arrival(&[GROUP])], true);
        assert_eq!(popups[0].title.as_ref(), "Clube do Livro");
        assert_eq!(popups[0].body.as_ref(), "Ana: Hi");
    }

    #[test]
    fn hides_the_message_without_previews() {
        let popups = directory().popups(&[&arrival(&[GROUP])], false);
        assert_eq!(popups[0].body.as_ref(), "New message");
    }

    #[test]
    fn shows_one_notification_per_chat_with_the_newest_message() {
        let mut newest = arrival(&[PERSON]);
        newest.summary = "Are you there?".into();
        let (first, group) = (arrival(&[PERSON]), arrival(&[GROUP]));
        let popups = directory().popups(&[&first, &group, &newest], true);
        assert_eq!(popups.len(), 2);
        assert_eq!(popups[0].title.as_ref(), "Ana Souza · 2 messages");
        assert_eq!(popups[0].body.as_ref(), "Are you there?");
    }

    #[test]
    fn keeps_text_that_looks_like_markup() {
        let mut code = arrival(&[PERSON]);
        code.summary = "if a < b && b > c".into();
        let popups = directory().popups(&[&code], true);
        assert_eq!(popups[0].body.as_ref(), "if a &lt; b &amp;&amp; b &gt; c");
    }
}
