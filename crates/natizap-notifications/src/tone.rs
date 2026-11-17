//! natizap's notification sounds, drawn in code so no recording has to be
//! bundled or licensed.

use std::f32::consts::TAU;

/// Samples per second the tones are drawn at.
pub(crate) const RATE: u32 = 48_000;
/// [`RATE`] for drawing.
const RATE_F32: f32 = 48_000.0;

/// A notification sound to choose from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Tone {
    /// A short rising "bloop", like a bubble popping.
    #[default]
    Bubble,
    /// Two soft wooden notes going down.
    Marimba,
    /// One warm, slow note, like a felt piano key.
    Soft,
}

impl Tone {
    /// Every tone, in the order they are offered.
    pub const ALL: [Self; 3] = [Self::Bubble, Self::Marimba, Self::Soft];

    /// The tone's name, as the settings show it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Bubble => "Bubble",
            Self::Marimba => "Marimba",
            Self::Soft => "Soft",
        }
    }

    /// How the preferences file spells the tone.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Bubble => "bubble",
            Self::Marimba => "marimba",
            Self::Soft => "soft",
        }
    }

    /// The tone the preferences file spells `key`.
    pub(crate) fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tone| tone.key() == key)
    }

    /// The tone, mono, [`RATE`] samples per second, peaking at the same
    /// loudness as the others.
    pub(crate) fn samples(self) -> Vec<f32> {
        let (length, fade_out) = match self {
            Self::Bubble => (0.22, 0.03),
            Self::Marimba => (0.5, 0.03),
            Self::Soft => (0.7, 0.08),
        };
        let mut phase = 0.0_f32;
        let samples: Vec<f32> = times(length)
            .map(|time| {
                let sound = match self {
                    Self::Bubble => {
                        // The pitch glides up an octave, which is what makes
                        // it a bubble rather than a beep.
                        let pitch = 380.0 + 380.0 * (1.0 - (-time / 0.025).exp());
                        phase += TAU * pitch / RATE_F32;
                        rise(time, 0.003) * (-time / 0.055).exp() * phase.sin()
                    }
                    Self::Marimba => [(659.25, 0.0), (493.88, 0.11)]
                        .iter()
                        .filter(|(_, start)| time >= *start)
                        .map(|&(pitch, start)| marimba(pitch, time - start))
                        .sum(),
                    Self::Soft => {
                        rise(time, 0.012)
                            * (-time / 0.18).exp()
                            * (sine(587.33, time) + 0.06 * sine(1174.66, time))
                    }
                };
                // Fading to silence at the end, so it never clicks.
                sound * ((length - time) / fade_out).min(1.0)
            })
            .collect();
        normalized(samples)
    }
}

/// A wooden bar struck: the note, and an overtone near its fourth harmonic
/// that dies away first, as a marimba's does.
fn marimba(pitch: f32, since: f32) -> f32 {
    rise(since, 0.002)
        * ((-since / 0.12).exp() * sine(pitch, since)
            + 0.25 * (-since / 0.015).exp() * sine(pitch * 3.93, since))
}

fn sine(pitch: f32, time: f32) -> f32 {
    (TAU * pitch * time).sin()
}

/// The ramp at a sound's start, which stops it clicking.
fn rise(time: f32, attack: f32) -> f32 {
    (time / attack).min(1.0)
}

/// Each sample's time, in seconds, over `length` seconds.
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "tones are under a second long"
)]
fn times(length: f32) -> impl Iterator<Item = f32> {
    (0..(length * RATE_F32) as usize).map(|index| index as f32 / RATE_F32)
}

/// `samples` scaled to peak at a loudness heard over music without
/// startling.
fn normalized(mut samples: Vec<f32>) -> Vec<f32> {
    const PEAK: f32 = 0.5;
    let loudest = samples
        .iter()
        .fold(0.0_f32, |loudest, sample| loudest.max(sample.abs()));
    if loudest > 0.0 {
        for sample in &mut samples {
            *sample *= PEAK / loudest;
        }
    }
    samples
}

#[cfg(test)]
mod tests {
    use super::{RATE, Tone};

    #[test]
    fn every_tone_peaks_at_the_same_loudness() {
        for tone in Tone::ALL {
            let loudest = tone.samples().iter().fold(0.0_f32, |l, s| l.max(s.abs()));
            assert!((loudest - 0.5).abs() < 1e-3, "{tone:?} peaks at {loudest}");
        }
    }

    #[test]
    fn every_tone_starts_and_ends_silent_so_it_never_clicks() {
        for tone in Tone::ALL {
            let samples = tone.samples();
            assert!(samples[0].abs() < 1e-3, "{tone:?} starts loud");
            assert!(
                samples[samples.len() - 1].abs() < 1e-3,
                "{tone:?} ends loud"
            );
        }
    }

    #[test]
    fn every_tone_is_shorter_than_a_second() {
        for tone in Tone::ALL {
            assert!(tone.samples().len() < RATE as usize, "{tone:?} is too long");
        }
    }

    #[test]
    fn every_tone_reads_back_from_its_key() {
        for tone in Tone::ALL {
            assert_eq!(Tone::from_key(tone.key()), Some(tone));
        }
    }
}
