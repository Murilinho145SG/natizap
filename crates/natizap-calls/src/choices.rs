//! What the user chose for calls: the ringtone, the microphone, speaker
//! and camera, how loud each person plays, and where the ringing card
//! sits. Kept in a small text file
//! of `key=value` lines, so it reads back across versions.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use gpui::{App, AppContext as _};
use natizap_playback::CallDevices;

use crate::ringtone::Ringtone;
use crate::volume::Loudness;

/// Where a person's loudness is kept: this, then their address.
const LOUDNESS: &str = "loudness.";

/// The user's choices for calls.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Choices {
    pub(crate) ringtone: Ringtone,
    pub(crate) devices: CallDevices,
    /// How loud people play, by address; as sent for everyone else.
    pub(crate) loudness: BTreeMap<String, Loudness>,
    /// Where the user put the ringing card, as fractions of the room it
    /// has across and down; the middle until they move it.
    pub(crate) ring_card: Option<(f32, f32)>,
}

impl Choices {
    /// What the file at `path` holds, or the defaults when it holds nothing
    /// or cannot be read.
    pub(crate) fn read(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(error) => {
                if error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(%error, "could not read the call settings");
                }
                Self::default()
            }
        }
    }

    /// Keeps these choices in the file at `path`, off the UI thread.
    pub(crate) fn write(&self, path: PathBuf, cx: &App) {
        let text = self.format();
        cx.background_spawn(async move {
            if let Err(error) = std::fs::write(&path, text) {
                tracing::warn!(%error, "could not keep the call settings");
            }
        })
        .detach();
    }

    /// The file's `text`. Unknown lines, such as ones a newer version
    /// wrote, are skipped, and missing ones keep their default.
    fn parse(text: &str) -> Self {
        let mut choices = Self::default();
        // Only the first `=` splits: device keys hold some themselves.
        for (key, value) in text
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.trim(), value.trim()))
            .filter(|(_, value)| !value.is_empty())
        {
            match key {
                "ringtone" => {
                    if let Some(ringtone) = Ringtone::from_key(value) {
                        choices.ringtone = ringtone;
                    }
                }
                "microphone" => choices.devices.microphone = Some(value.to_owned()),
                "speaker" => choices.devices.speaker = Some(value.to_owned()),
                "camera" => choices.devices.camera = Some(value.to_owned()),
                "ring_card" => choices.ring_card = fractions(value),
                _ => {
                    if let (Some(person), Ok(percent)) =
                        (key.strip_prefix(LOUDNESS), value.parse::<u16>())
                    {
                        choices
                            .loudness
                            .insert(person.to_owned(), Loudness::new(percent));
                    }
                }
            }
        }
        choices
    }

    fn format(&self) -> String {
        let mut text = format!("ringtone={}\n", self.ringtone.key());
        let devices = &self.devices;
        for (key, device) in [
            ("microphone", &devices.microphone),
            ("speaker", &devices.speaker),
            ("camera", &devices.camera),
        ] {
            if let Some(device) = device {
                // Writing to a String cannot fail.
                let _ = writeln!(text, "{key}={device}");
            }
        }
        if let Some((across, down)) = self.ring_card {
            let _ = writeln!(text, "ring_card={across},{down}");
        }
        for (person, loudness) in &self.loudness {
            if *loudness != Loudness::SENT {
                let _ = writeln!(text, "{LOUDNESS}{person}={}", loudness.percent());
            }
        }
        text
    }
}

/// Two fractions, `across,down`, each from 0 to 1.
fn fractions(value: &str) -> Option<(f32, f32)> {
    let (across, down) = value.split_once(',')?;
    let fraction = |text: &str| {
        text.trim()
            .parse::<f32>()
            .ok()
            .filter(|fraction| (0.0..=1.0).contains(fraction))
    };
    Some((fraction(across)?, fraction(down)?))
}

#[cfg(test)]
mod tests {
    use natizap_playback::CallDevices;

    use super::Choices;
    use crate::ringtone::Ringtone;
    use crate::volume::Loudness;

    #[test]
    fn reads_back_what_it_wrote() {
        let choices = Choices {
            ringtone: Ringtone::Guitar,
            devices: CallDevices {
                microphone: Some("pulseaudio:alsa_input.usb-fifine.mono".into()),
                speaker: None,
                camera: Some("/dev/video2".into()),
            },
            loudness: [(
                "5511999990000@s.whatsapp.net".to_owned(),
                Loudness::new(150),
            )]
            .into(),
            ring_card: Some((0.25, 1.0)),
        };
        assert_eq!(Choices::parse(&choices.format()), choices);
    }

    #[test]
    fn keeps_device_keys_that_hold_an_equals_sign() {
        let choices = Choices::parse("speaker=alsa:hw:CARD=PCH,DEV=0\n");
        assert_eq!(
            choices.devices.speaker.as_deref(),
            Some("alsa:hw:CARD=PCH,DEV=0")
        );
    }

    #[test]
    fn keeps_the_defaults_for_what_the_file_lacks_or_garbles() {
        assert_eq!(Choices::parse(""), Choices::default());
        let garbled = Choices::parse("ringtone=trumpet\nvolume=11\n");
        assert_eq!(garbled, Choices::default());
        assert_eq!(
            Choices::parse("volume=11\nringtone = guitar\n").ringtone,
            Ringtone::Guitar
        );
    }
}
