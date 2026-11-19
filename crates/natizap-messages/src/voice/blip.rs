//! The short blip between two voice notes played one after the other, as
//! the phone sounds it, drawn in code so no recording is bundled.

use std::f32::consts::TAU;
use std::time::Duration;

use natizap_playback::Clip;

/// Samples per second the blip is drawn at.
const RATE: u32 = 48_000;
const RATE_F32: f32 = 48_000.0;
/// How long it lasts, in seconds.
const LENGTH: f32 = 0.09;
/// It rises from this pitch to the next, in hertz.
const FROM: f32 = 990.0;
const TO: f32 = 1480.0;
/// Quieter than a notification: it plays over the user's listening.
const LOUDNESS: f32 = 0.18;

/// The blip, mono, [`RATE`] samples per second.
fn samples() -> Vec<f32> {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a fraction of a second of samples, far inside usize"
    )]
    let count = (LENGTH * RATE_F32) as usize;
    let mut phase = 0.0_f32;
    (0..count)
        .map(|index| {
            #[expect(
                clippy::cast_precision_loss,
                reason = "sample indices of a short blip are exact in f32"
            )]
            let t = index as f32 / RATE_F32;
            let progress = t / LENGTH;
            phase += TAU * (FROM + (TO - FROM) * progress) / RATE_F32;
            // A quick attack and a smooth fall, so it does not click.
            let envelope = (t / 0.004).min(1.0) * (1.0 - progress).powi(2);
            phase.sin() * envelope * LOUDNESS
        })
        .collect()
}

/// Plays the blip without waiting for it.
pub(crate) fn play() {
    // A thread of its own: opening the sound card can take a moment, and
    // the clip stops once dropped.
    let playing = std::thread::Builder::new()
        .name("natizap-voice-blip".into())
        .spawn(|| match Clip::play(&samples(), RATE) {
            Ok(clip) => std::thread::sleep(clip.length() + Duration::from_millis(50)),
            Err(error) => tracing::warn!(%error, "could not play the next-note blip"),
        });
    if let Err(error) = playing {
        tracing::warn!(%error, "could not start the next-note blip");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_short_quiet_and_fades_to_silence() {
        let blip = samples();
        assert!(blip.len() > 1000);
        assert!(blip.iter().all(|sample| sample.abs() <= LOUDNESS));
        assert!(blip.last().is_some_and(|last| last.abs() < 0.01));
    }
}
