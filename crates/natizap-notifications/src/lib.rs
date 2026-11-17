//! Notifications: a chime when a message arrives that the user is not
//! looking at, as the phone sounds for messages outside the open chat, and
//! the settings that choose when and with what.
//!
//! [`Notifier`] hears new messages itself; the binary tells it what is on
//! screen ([`Notifier::set_focused`], [`Notifier::set_open`]) and which
//! chats are muted ([`Notifier::set_muted`]). [`NotificationSettings`] is
//! the dialog that changes its [`Preferences`].

mod preferences;
mod settings;
mod tone;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{Context, Entity, Task};
use natizap_playback::Clip;
use natizap_protocol::whatsapp_rust::prelude::{Event, EventKind};
use natizap_protocol::whatsapp_rust::types::events::InboundMessage;
use natizap_protocol::whatsapp_rust::wacore_binary::JidExt as _;
use natizap_protocol::{ChatId, Content, Protocol};

pub use preferences::{Alert, Preferences};
pub use settings::{NotificationSettings, SettingsClosed};
pub use tone::Tone;

/// The shortest gap between two chimes: a burst of messages, such as those
/// that queued up while offline, chimes once.
const QUIET_AFTER_CHIME: Duration = Duration::from_millis(1500);

/// Sounds a chime for messages that arrive where the user is not looking.
/// Notifies observers when its preferences change.
pub struct Notifier {
    preferences: Preferences,
    /// Where the preferences are kept.
    path: PathBuf,
    quiet: Quiet,
    chimed: Option<Instant>,
    tones: Vec<(Tone, Arc<[f32]>)>,
    _tasks: [Task<()>; 2],
}

impl std::fmt::Debug for Notifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Notifier")
            .field("preferences", &self.preferences)
            .field("quiet", &self.quiet)
            .finish_non_exhaustive()
    }
}

impl Notifier {
    /// Listens for messages arriving through `protocol`, with the
    /// preferences kept at `path`.
    pub fn new(protocol: &Entity<Protocol>, path: PathBuf, cx: &mut Context<Self>) -> Self {
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
        let reading = path.clone();
        let loading = cx.spawn(async move |this, cx| {
            let text = cx
                .background_spawn(async move { std::fs::read_to_string(reading) })
                .await;
            let preferences = match text {
                Ok(text) => Preferences::parse(&text),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
                Err(error) => {
                    tracing::warn!(%error, "could not read the notification settings");
                    return;
                }
            };
            let _ = this.update(cx, |notifier, cx| {
                notifier.preferences = preferences;
                cx.notify();
            });
        });
        Self {
            preferences: Preferences::default(),
            path,
            quiet: Quiet::default(),
            chimed: None,
            tones: Tone::ALL
                .into_iter()
                .map(|tone| (tone, tone.samples().into()))
                .collect(),
            _tasks: [hearing, loading],
        }
    }

    /// How natizap notifies.
    #[must_use]
    pub fn preferences(&self) -> Preferences {
        self.preferences
    }

    /// Changes how natizap notifies, and keeps the change on disk.
    pub fn set_preferences(&mut self, preferences: Preferences, cx: &mut Context<Self>) {
        if self.preferences == preferences {
            return;
        }
        self.preferences = preferences;
        cx.notify();
        let (path, text) = (self.path.clone(), preferences.format());
        cx.background_spawn(async move {
            if let Err(error) = std::fs::write(&path, text) {
                tracing::warn!(%error, "could not keep the notification settings");
            }
        })
        .detach();
    }

    /// Plays `tone`, so the user can hear it before choosing it.
    pub fn preview(&self, tone: Tone) {
        self.play(tone);
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
        let Some(tone) = arrivals
            .iter()
            .find_map(|arrival| self.quiet.tone_for(arrival, self.preferences))
        else {
            return;
        };
        let now = Instant::now();
        if self
            .chimed
            .is_some_and(|chimed| now.duration_since(chimed) < QUIET_AFTER_CHIME)
        {
            return;
        }
        self.chimed = Some(now);
        self.play(tone);
    }

    fn play(&self, tone: Tone) {
        let Some(samples) = self
            .tones
            .iter()
            .find(|(drawn, _)| *drawn == tone)
            .map(|(_, samples)| Arc::clone(samples))
        else {
            return;
        };
        // A thread of its own: opening the sound card can take a moment,
        // and the card's stream must live until the tone has played.
        let playing = std::thread::Builder::new()
            .name("natizap-chime".into())
            .spawn(move || match Clip::play(&samples, tone::RATE) {
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
    group: bool,
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
    Some(Arrival {
        chat,
        group: source.is_group,
    })
}

/// What is on screen and what is muted.
#[derive(Debug, Default)]
struct Quiet {
    focused: bool,
    open: HashSet<ChatId>,
    muted: HashSet<ChatId>,
}

impl Quiet {
    /// The tone `arrival` chimes with, if any: never in a muted chat or
    /// the chat the user is looking at, and in other chats while natizap
    /// is in use only when `preferences` say so.
    fn tone_for(&self, arrival: &Arrival, preferences: Preferences) -> Option<Tone> {
        let alert = if arrival.group {
            preferences.groups
        } else {
            preferences.messages
        };
        let any_of =
            |addresses: &HashSet<ChatId>| arrival.chat.iter().any(|id| addresses.contains(id));
        let silenced = !alert.sound
            || any_of(&self.muted)
            || self.focused && (any_of(&self.open) || !preferences.other_chats);
        (!silenced).then_some(alert.tone)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use natizap_protocol::ChatId;

    use super::{Alert, Arrival, Preferences, Quiet, Tone};

    const PERSON: &str = "5511900000001@s.whatsapp.net";
    const OTHER: &str = "5511900000002@s.whatsapp.net";
    const GROUP: &str = "120363000000000001@g.us";

    fn arrival(ids: &[&str]) -> Arrival {
        Arrival {
            chat: ids.iter().copied().map(ChatId::new).collect(),
            group: ids.iter().any(|id| id.ends_with("@g.us")),
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
}
