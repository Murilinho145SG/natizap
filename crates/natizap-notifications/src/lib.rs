//! Notifications: a chime when a message arrives that the user is not
//! looking at, as the phone sounds for messages outside the open chat, a
//! system notification while natizap is in the background, and the
//! settings that choose when and how.
//!
//! [`Notifier`] hears new messages itself; the binary tells it what is on
//! screen ([`Notifier::set_focused`], [`Notifier::set_open`]), which chats
//! are muted ([`Notifier::set_muted`]) and what chats and people are
//! called ([`Notifier::set_titles`], [`Notifier::set_names`]).
//! [`NotificationSettings`] is the settings page that changes its
//! [`Preferences`].

mod heard;
mod popup;
mod preferences;
mod settings;
mod tone;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{Context, Entity, EventEmitter, SharedString, Task};
use natizap_playback::Clip;
use natizap_protocol::whatsapp_rust::prelude::EventKind;
use natizap_protocol::{ChatId, PersonName, Protocol};

use crate::heard::{Arrival, Quiet, arrivals};
use crate::popup::{AWAY_TAG, Directory};

pub use preferences::{Alert, Preferences};
pub use settings::NotificationSettings;
pub use tone::Tone;

/// The shortest gap between two chimes: a burst of messages, such as those
/// that queued up while offline, chimes once.
const QUIET_AFTER_CHIME: Duration = Duration::from_millis(1500);
/// The shortest gap between two notifications about messages queued while
/// offline, which may arrive over several batches.
const QUIET_AFTER_AWAY: Duration = Duration::from_secs(60);

/// The user clicked a system notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationOpened {
    /// The chat it was about; `None` for one about several chats.
    pub chat: Option<ChatId>,
}

/// Sounds a chime for messages that arrive where the user is not looking,
/// and shows them in the system's notifications while natizap is in the
/// background. Notifies observers when its preferences change, and emits
/// [`NotificationOpened`].
pub struct Notifier {
    preferences: Preferences,
    /// Where the preferences are kept.
    path: PathBuf,
    quiet: Quiet,
    directory: Directory,
    chimed: Option<Instant>,
    /// When the user was last told of messages queued while offline.
    told_away: Option<Instant>,
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

impl EventEmitter<NotificationOpened> for Notifier {}

impl Notifier {
    /// Listens for messages arriving through `protocol`, with the
    /// preferences kept at `path`.
    pub fn new(protocol: &Entity<Protocol>, path: PathBuf, cx: &mut Context<Self>) -> Self {
        let events = protocol.read(cx).subscribe(&[EventKind::Messages]);
        let hearing = cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                let arrivals = arrivals(&event);
                if arrivals.is_empty() {
                    let messages = event.messages().count();
                    if messages > 0 {
                        tracing::debug!(messages, "messages heard, none to notify about");
                    }
                    continue;
                }
                if this
                    .update(cx, |notifier, cx| notifier.hear(&arrivals, cx))
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
        let opened = cx.weak_entity();
        // The app has one handler for every notification, and natizap
        // posts none but these.
        cx.on_system_notification_response(move |response, cx| {
            let chat = (response.tag.as_ref() != AWAY_TAG && !response.tag.is_empty())
                .then(|| ChatId::new(&response.tag));
            let _ = opened.update(cx, |_, cx| cx.emit(NotificationOpened { chat }));
        });
        Self {
            preferences: Preferences::default(),
            path,
            quiet: Quiet::default(),
            directory: Directory::default(),
            chimed: None,
            told_away: None,
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

    /// Each chat's title, by every address it is known under.
    pub fn set_titles(&mut self, titles: HashMap<ChatId, SharedString>) {
        self.directory.titles = titles;
    }

    /// Everyone's names, by every address they are known under.
    pub fn set_names(&mut self, names: Arc<HashMap<ChatId, PersonName>>) {
        self.directory.names = names;
    }

    fn hear(&mut self, arrivals: &[Arrival], cx: &mut Context<Self>) {
        // Why a message did or did not chime is otherwise invisible.
        for arrival in arrivals {
            tracing::debug!(
                chat = ?arrival.chat,
                group = arrival.group,
                offline = arrival.offline,
                focused = self.quiet.focused,
                muted = self.quiet.is_muted(arrival),
                open = self.quiet.is_open(arrival),
                tone = ?self.quiet.tone_for(arrival, self.preferences),
                popup = self.quiet.shows(arrival, self.preferences),
                "message heard"
            );
        }
        self.notify(arrivals, cx);
        self.chime(arrivals);
    }

    /// Shows `arrivals` in the system's notifications, as far as they
    /// deserve it.
    fn notify(&mut self, arrivals: &[Arrival], cx: &mut Context<Self>) {
        let (offline, live): (Vec<&Arrival>, Vec<&Arrival>) = arrivals
            .iter()
            .filter(|arrival| self.quiet.shows(arrival, self.preferences))
            .partition(|arrival| arrival.offline);
        for popup in self.directory.popups(&live, self.preferences.previews) {
            cx.show_system_notification(popup);
        }
        let now = Instant::now();
        let told = self
            .told_away
            .is_some_and(|told| now.duration_since(told) < QUIET_AFTER_AWAY);
        if !offline.is_empty() && !told {
            self.told_away = Some(now);
            cx.show_system_notification(popup::away());
        }
    }

    fn chime(&mut self, arrivals: &[Arrival]) {
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
