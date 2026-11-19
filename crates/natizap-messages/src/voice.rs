//! The voice note or audio file playing inside its bubble.

use std::path::Path;
use std::time::Duration;

use gpui::SharedString;
use natizap_playback::{Playback, Speed};

pub(crate) mod blip;

/// What a bubble shows about the note playing in it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct VoiceState {
    pub(crate) message_id: SharedString,
    pub(crate) position: Duration,
    /// Share already played, from 0 to 1.
    pub(crate) progress: f32,
    pub(crate) playing: bool,
    /// The file is still downloading.
    pub(crate) loading: bool,
}

/// What every voice bubble needs to draw itself.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct VoiceUi {
    /// The note playing, if any.
    pub(crate) playing: Option<VoiceState>,
    /// The speed every note plays at.
    pub(crate) speed: Speed,
    /// The bubble whose speed menu is open.
    pub(crate) speed_menu: Option<SharedString>,
}

/// The note playing in the open chat. One plays at a time, as on the
/// phone, and the chosen speed carries over to the next one.
#[derive(Debug, Default)]
pub(crate) struct Voices {
    current: Option<Current>,
    speed: Speed,
    speed_menu: Option<SharedString>,
}

#[derive(Debug)]
struct Current {
    message_id: SharedString,
    /// `None` while the file downloads.
    playback: Option<Playback>,
    paused: bool,
}

impl Voices {
    /// Plays or pauses the note of `message_id`, stopping any other.
    /// Returns whether its file has to be downloaded first; it starts once
    /// [`Self::downloaded`] reports it.
    pub(crate) fn toggle(&mut self, message_id: &SharedString, file: Option<&Path>) -> bool {
        if let Some(current) = self
            .current
            .as_mut()
            .filter(|current| current.message_id == *message_id)
        {
            if let Some(playback) = &current.playback {
                current.paused = !current.paused;
                playback.set_paused(current.paused);
            }
            return false;
        }
        let playback = file
            .filter(|file| file.exists())
            .map(|file| start(file, self.speed));
        let must_download = playback.is_none();
        self.current = Some(Current {
            message_id: message_id.clone(),
            playback,
            paused: false,
        });
        must_download
    }

    /// The file of `message_id` is on disk: plays it if it is still the
    /// note waiting for it.
    pub(crate) fn downloaded(&mut self, message_id: &str, file: &Path) {
        let speed = self.speed;
        if let Some(current) = self
            .current
            .as_mut()
            .filter(|current| current.message_id == message_id && current.playback.is_none())
        {
            current.playback = Some(start(file, speed));
        }
    }

    /// Plays this note and the ones after it at `speed`, closing the menu.
    pub(crate) fn set_speed(&mut self, speed: Speed) {
        self.speed = speed;
        self.speed_menu = None;
        if let Some(playback) = self.playback() {
            playback.set_speed(speed);
        }
    }

    /// Opens the speed menu of `message_id`'s bubble, or closes it if open.
    pub(crate) fn toggle_speed_menu(&mut self, message_id: &SharedString) {
        self.speed_menu = if self.speed_menu.as_ref() == Some(message_id) {
            None
        } else {
            Some(message_id.clone())
        };
    }

    /// Closes the speed menu; returns whether it was open.
    pub(crate) fn close_speed_menu(&mut self) -> bool {
        self.speed_menu.take().is_some()
    }

    /// Jumps to `fraction` of the note of `message_id`, if it is playing.
    pub(crate) fn seek(&self, message_id: &str, fraction: f32) {
        let Some(playback) = self.playback().filter(|_| {
            self.current
                .as_ref()
                .is_some_and(|c| c.message_id == message_id)
        }) else {
            return;
        };
        if let Some(total) = playback.status().duration {
            playback.seek(total.mul_f32(fraction.clamp(0.0, 1.0)));
        }
    }

    /// The note that just played to its end, if one did, which stops
    /// here; one that failed to play does not count.
    pub(crate) fn finished(&mut self) -> Option<SharedString> {
        let current = self.current.as_ref()?;
        let status = current.playback.as_ref()?.status();
        if !status.ended || status.error.is_some() {
            return None;
        }
        self.current.take().map(|current| current.message_id)
    }

    /// Stops playing, e.g. when another chat opens.
    pub(crate) fn stop(&mut self) {
        self.current = None;
        self.speed_menu = None;
    }

    /// What the bubbles show now. See [`Self::state`].
    pub(crate) fn ui(&mut self) -> VoiceUi {
        VoiceUi {
            playing: self.state(),
            speed: self.speed,
            speed_menu: self.speed_menu.clone(),
        }
    }

    /// The playing note as its bubble shows it. A note that played to the
    /// end, or could not be played, stops here and shows as not playing,
    /// as on the phone.
    fn state(&mut self) -> Option<VoiceState> {
        let current = self.current.as_ref()?;
        let Some(playback) = &current.playback else {
            return Some(VoiceState {
                message_id: current.message_id.clone(),
                position: Duration::ZERO,
                progress: 0.0,
                playing: false,
                loading: true,
            });
        };
        let status = playback.status();
        // The decoder already logged why a file could not be played.
        if status.ended || status.error.is_some() {
            self.current = None;
            return None;
        }
        let position = playback.position();
        let progress = status
            .duration
            .filter(|total| !total.is_zero())
            .map_or(0.0, |total| {
                (position.as_secs_f32() / total.as_secs_f32()).clamp(0.0, 1.0)
            });
        Some(VoiceState {
            message_id: current.message_id.clone(),
            position,
            progress,
            playing: !current.paused,
            loading: false,
        })
    }

    /// Whether what the bubble shows changes as time passes.
    pub(crate) fn is_running(&self) -> bool {
        self.current
            .as_ref()
            .is_some_and(|current| current.playback.is_none() || !current.paused)
    }

    fn playback(&self) -> Option<&Playback> {
        self.current.as_ref()?.playback.as_ref()
    }
}

fn start(file: &Path, speed: Speed) -> Playback {
    let playback = Playback::start(file);
    if speed != Speed::Normal {
        playback.set_speed(speed);
    }
    playback
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_not_on_disk_waits_for_its_download() {
        let mut voices = Voices::default();
        let id = SharedString::from("a");
        assert!(voices.toggle(&id, None));
        let state = voices
            .ui()
            .playing
            .map(|state| (state.message_id, state.loading));
        assert_eq!(state, Some((id, true)));
        assert!(voices.is_running());
    }

    #[test]
    fn a_download_finishing_late_does_not_take_over_the_newer_note() {
        let mut voices = Voices::default();
        voices.toggle(&"a".into(), None);
        voices.toggle(&"b".into(), None);
        voices.downloaded("a", Path::new("/nonexistent/a.ogg"));
        let state = voices
            .ui()
            .playing
            .map(|state| (state.message_id, state.loading));
        assert_eq!(state, Some(("b".into(), true)));
    }

    #[test]
    fn the_chosen_speed_outlives_the_note_and_closes_the_menu() {
        let mut voices = Voices::default();
        voices.toggle(&"a".into(), None);
        voices.toggle_speed_menu(&"a".into());
        voices.set_speed(Speed::Fast);
        voices.toggle(&"b".into(), None);
        let ui = voices.ui();
        assert_eq!((ui.speed, ui.speed_menu), (Speed::Fast, None));
    }

    #[test]
    fn one_speed_menu_is_open_at_a_time() {
        let mut voices = Voices::default();
        voices.toggle_speed_menu(&"a".into());
        voices.toggle_speed_menu(&"b".into());
        assert_eq!(voices.ui().speed_menu, Some("b".into()));
        voices.toggle_speed_menu(&"b".into());
        assert!(!voices.close_speed_menu());
    }
}
