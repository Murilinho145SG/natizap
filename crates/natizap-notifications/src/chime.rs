//! natizap's own notification sound, drawn in code so no recording has to
//! be bundled or licensed.

use std::f32::consts::TAU;

/// Samples per second the chime is drawn at.
pub(crate) const RATE: u32 = 48_000;

/// Two soft notes a fourth apart, the second starting as the first fades.
const NOTES: [(f32, f32); 2] = [(783.99, 0.0), (1046.5, 0.085)];
/// How long the chime plays, in seconds.
const LENGTH: f32 = 0.42;
/// How fast each note fades: its loudness falls to 1/e in this many seconds.
const FADE: f32 = 0.075;
/// The rise at a note's start, which stops it clicking.
const ATTACK: f32 = 0.004;
/// Loud enough to hear over music, quiet enough not to startle.
const LOUDNESS: f32 = 0.22;

/// The chime, mono, [`RATE`] samples per second.
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the chime is a fraction of a second long"
)]
pub(crate) fn chime() -> Vec<f32> {
    let rate = RATE as f32;
    let count = (LENGTH * rate) as usize;
    (0..count)
        .map(|index| {
            let time = index as f32 / rate;
            let sound: f32 = NOTES
                .iter()
                .filter(|(_, start)| time >= *start)
                .map(|&(pitch, start)| {
                    let since = time - start;
                    let rise = (since / ATTACK).min(1.0);
                    let fade = (-since / FADE).exp();
                    // A touch of the octave above makes it ring like a bell
                    // instead of beeping like a test tone.
                    let tone =
                        (TAU * pitch * since).sin() + 0.18 * (TAU * 2.0 * pitch * since).sin();
                    rise * fade * tone
                })
                .sum();
            // The tail fades to silence, so the end never clicks either.
            let tail = ((LENGTH - time) / 0.03).min(1.0);
            sound * tail * LOUDNESS
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{RATE, chime};

    #[test]
    fn stays_within_full_scale() {
        assert!(chime().iter().all(|sample| sample.abs() < 1.0));
    }

    #[test]
    fn starts_and_ends_silent_so_it_never_clicks() {
        let chime = chime();
        assert!(chime[0].abs() < 1e-3);
        assert!(chime[chime.len() - 1].abs() < 1e-3);
    }

    #[test]
    fn lasts_under_half_a_second() {
        let samples = chime().len();
        assert!(samples > 0 && samples < RATE as usize / 2);
    }
}
