//! What the user chose about notifications, kept in a small text file of
//! `key=value` lines so it reads back across versions.

use crate::Tone;

/// How natizap notifies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preferences {
    /// Messages in chats with one person.
    pub messages: Alert,
    /// Messages in groups.
    pub groups: Alert,
    /// Whether messages in other chats chime while natizap is in use.
    pub other_chats: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            messages: Alert::default(),
            groups: Alert::default(),
            other_chats: true,
        }
    }
}

/// Whether one kind of message chimes, and with what.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Alert {
    /// Whether it chimes at all.
    pub sound: bool,
    /// The chime.
    pub tone: Tone,
}

impl Default for Alert {
    fn default() -> Self {
        Self {
            sound: true,
            tone: Tone::default(),
        }
    }
}

impl Preferences {
    /// Reads the file's `text`. Unknown lines, such as ones a newer version
    /// wrote, are skipped, and missing ones keep their default.
    pub(crate) fn parse(text: &str) -> Self {
        let mut preferences = Self::default();
        for (key, value) in text
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.trim(), value.trim()))
        {
            let on = match value {
                "on" => Some(true),
                "off" => Some(false),
                _ => None,
            };
            match (key, on) {
                ("messages", Some(on)) => preferences.messages.sound = on,
                ("groups", Some(on)) => preferences.groups.sound = on,
                ("other_chats", Some(on)) => preferences.other_chats = on,
                ("messages_tone", _) => {
                    if let Some(tone) = Tone::from_key(value) {
                        preferences.messages.tone = tone;
                    }
                }
                ("groups_tone", _) => {
                    if let Some(tone) = Tone::from_key(value) {
                        preferences.groups.tone = tone;
                    }
                }
                _ => {}
            }
        }
        preferences
    }

    /// The file's text.
    pub(crate) fn format(self) -> String {
        let on = |on: bool| if on { "on" } else { "off" };
        format!(
            "messages={}\nmessages_tone={}\ngroups={}\ngroups_tone={}\nother_chats={}\n",
            on(self.messages.sound),
            self.messages.tone.key(),
            on(self.groups.sound),
            self.groups.tone.key(),
            on(self.other_chats),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Alert, Preferences};
    use crate::Tone;

    #[test]
    fn reads_back_what_it_wrote() {
        let preferences = Preferences {
            messages: Alert {
                sound: false,
                tone: Tone::Marimba,
            },
            groups: Alert {
                sound: true,
                tone: Tone::Soft,
            },
            other_chats: false,
        };
        assert_eq!(Preferences::parse(&preferences.format()), preferences);
    }

    #[test]
    fn keeps_defaults_for_what_the_file_lacks_or_garbles() {
        let preferences = Preferences::parse("groups=off\nmessages_tone=kazoo\nvolume=11\n");
        assert!(!preferences.groups.sound);
        assert_eq!(preferences.messages, Alert::default());
        assert!(preferences.other_chats);
    }
}
