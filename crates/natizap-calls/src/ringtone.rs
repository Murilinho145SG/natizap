//! The sounds of a call ringing, drawn in code so no recording has to be
//! bundled or licensed: the ringtone of a call coming in, and the ringback
//! the caller hears while the other phone rings.

use std::f32::consts::TAU;
use std::sync::Arc;
use std::time::Duration;

use gpui::{App, Task};
use natizap_playback::Clip;

/// Samples per second the sounds are drawn at.
const RATE: u32 = 48_000;
const RATE_F32: f32 = 48_000.0;

/// A call's ringing sound, repeated until dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ringtone {
    /// A call coming in: three soft rising notes.
    Incoming,
    /// The other phone ringing: one low tone, then quiet.
    Ringback,
}

impl Ringtone {
    /// How often the sound starts again.
    fn every(self) -> Duration {
        match self {
            Self::Incoming => Duration::from_millis(2_400),
            Self::Ringback => Duration::from_millis(4_000),
        }
    }

    /// The sound once, mono, at [`RATE`].
    fn samples(self) -> Vec<f32> {
        match self {
            // E5, G#5 and B5: a major chord, played upwards.
            Self::Incoming => [(659.3, 0.0), (830.6, 0.16), (987.8, 0.32)]
                .into_iter()
                .fold(vec![0.0; seconds(1.1)], |mut sound, (hertz, start)| {
                    for (index, sample) in sound.iter_mut().enumerate().skip(seconds(start)) {
                        let at = time(index) - start;
                        *sample += 0.18 * (TAU * hertz * at).sin() * (-at * 4.5).exp();
                    }
                    sound
                }),
            // 425 Hz, the ringback tone of most of the world, for a second.
            Self::Ringback => (0..seconds(1.0))
                .map(|index| {
                    let at = time(index);
                    let fade = (at / 0.02).min(1.0) * ((1.0 - at) / 0.02).min(1.0);
                    0.12 * (TAU * 425.0 * at).sin() * fade
                })
                .collect(),
        }
    }

    /// Plays the sound over and over until the task is dropped.
    pub(crate) fn ring(self, cx: &App) -> Task<()> {
        let sound: Arc<[f32]> = self.samples().into();
        let timer = cx.background_executor().clone();
        cx.spawn(async move |_| {
            loop {
                let clip = Clip::play(&sound, RATE);
                if let Err(error) = &clip {
                    tracing::warn!(%error, "could not ring");
                }
                timer.timer(self.every()).await;
                drop(clip);
            }
        })
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a second or so of samples"
)]
fn seconds(seconds: f32) -> usize {
    (seconds * RATE_F32) as usize
}

#[expect(clippy::cast_precision_loss, reason = "a second or so of samples")]
fn time(index: usize) -> f32 {
    index as f32 / RATE_F32
}

#[cfg(test)]
mod tests {
    use super::Ringtone;

    #[test]
    fn each_sound_ends_before_it_starts_again() {
        for tone in [Ringtone::Incoming, Ringtone::Ringback] {
            let length = super::time(tone.samples().len());
            assert!(length < tone.every().as_secs_f32(), "{tone:?}");
        }
    }

    #[test]
    fn neither_sound_clips() {
        for tone in [Ringtone::Incoming, Ringtone::Ringback] {
            let loudest = tone
                .samples()
                .iter()
                .fold(0.0_f32, |most, s| most.max(s.abs()));
            assert!(
                loudest > 0.05 && loudest < 1.0,
                "{tone:?} peaks at {loudest}"
            );
        }
    }
}
