//! The voice note or audio file playing inside its bubble.

use std::path::Path;
use std::time::Duration;

use gpui::SharedString;
use natizap_playback::{Playback, Speed};

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
    pub(crate) speed: Speed,
}

/// The note playing in the open chat. One plays at a time, as on the
/// phone, and the chosen speed carries over to the next one.
#[derive(Debug, Default)]
pub(crate) struct Voices {
    current: Option<Current>,
    speed: Speed,
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

    /// Switches to the next speed, for this note and the ones after it.
    pub(crate) fn cycle_speed(&mut self) {
        self.speed = self.speed.next();
        if let Some(playback) = self.playback() {
            playback.set_speed(self.speed);
        }
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

    /// Stops playing, e.g. when another chat opens.
    pub(crate) fn stop(&mut self) {
        self.current = None;
    }

    /// The playing note as its bubble shows it. A note that played to the
    /// end, or could not be played, stops here and shows as not playing,
    /// as on the phone.
    pub(crate) fn state(&mut self) -> Option<VoiceState> {
        let current = self.current.as_ref()?;
        let Some(playback) = &current.playback else {
            return Some(VoiceState {
                message_id: current.message_id.clone(),
                position: Duration::ZERO,
                progress: 0.0,
                playing: false,
                loading: true,
                speed: self.speed,
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
            speed: self.speed,
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
            .state()
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
            .state()
            .map(|state| (state.message_id, state.loading));
        assert_eq!(state, Some(("b".into(), true)));
    }

    #[test]
    fn the_chosen_speed_outlives_the_note() {
        let mut voices = Voices::default();
        voices.toggle(&"a".into(), None);
        voices.cycle_speed();
        voices.stop();
        voices.toggle(&"b".into(), None);
        assert_eq!(voices.state().map(|state| state.speed), Some(Speed::Faster));
    }
}
