//! The sounds of a call ringing, drawn in code so no recording has to be
//! bundled or licensed: the ringtone of a call coming in, one of a few to
//! choose from, and the ringback the caller hears while the other phone
//! rings, two soft knocks of the same electric piano. Each imitates a real instrument instead of a bare tone: modal
//! partials for the kalimba, a plucked string for the guitar, FM for the
//! electric piano, all through a small room.

use std::f32::consts::TAU;
use std::sync::Arc;
use std::time::Duration;

use gpui::{App, Task};
use natizap_playback::Clip;

/// Samples per second the sounds are drawn at.
pub(crate) const RATE: u32 = 48_000;
const RATE_F32: f32 = 48_000.0;
/// The loudest any sound peaks.
const PEAK: f32 = 0.55;

/// A ringtone for calls coming in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Ringtone {
    /// A pentatonic phrase on a kalimba, lifting at the end.
    #[default]
    Kalimba,
    /// A nylon guitar, fingerpicked.
    Guitar,
    /// Two soft chords on an electric piano.
    Piano,
}

impl Ringtone {
    /// Every ringtone, in the order they are offered.
    pub const ALL: [Self; 3] = [Self::Kalimba, Self::Guitar, Self::Piano];

    /// The ringtone's name, as the settings show it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Kalimba => "Kalimba",
            Self::Guitar => "Guitar",
            Self::Piano => "Electric piano",
        }
    }

    /// How the settings file spells it.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Kalimba => "kalimba",
            Self::Guitar => "guitar",
            Self::Piano => "piano",
        }
    }

    /// The ringtone the settings file spells `key`.
    pub(crate) fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tone| tone.key() == key)
    }
}

/// A sound a call rings with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ringing {
    /// A call coming in.
    Incoming(Ringtone),
    /// The other phone ringing.
    Ringback,
}

impl Ringing {
    /// How long one round lasts, its tail included; the next starts then.
    fn every(self) -> Duration {
        Duration::from_millis(match self {
            Self::Incoming(Ringtone::Kalimba) => 3_200,
            Self::Incoming(Ringtone::Guitar | Ringtone::Piano) => 3_600,
            Self::Ringback => 4_000,
        })
    }

    /// One round, mono, at [`RATE`].
    pub(crate) fn samples(self) -> Vec<f32> {
        let mut sound = vec![0.0; seconds(self.every().as_secs_f32())];
        match self {
            Self::Incoming(Ringtone::Kalimba) => {
                for (index, key) in [72, 76, 79, 76, 81, 79, 84].into_iter().enumerate() {
                    let loud = if index == 6 { 1.0 } else { 0.8 };
                    #[expect(clippy::cast_precision_loss, reason = "a few notes")]
                    mix(&mut sound, index as f32 * 0.17, &kalimba(pitch(key), loud));
                }
                room(&mut sound, 0.22);
            }
            Self::Incoming(Ringtone::Guitar) => {
                let notes = [
                    (52, 0.0),
                    (59, 0.14),
                    (64, 0.28),
                    (68, 0.42),
                    (71, 0.56),
                    (68, 0.70),
                    (73, 0.98),
                    (71, 1.12),
                ];
                for (index, (key, at)) in notes.into_iter().enumerate() {
                    let loud = if index == 0 { 1.0 } else { 0.9 };
                    mix(&mut sound, at, &pluck(pitch(key), loud, index));
                }
                soften(&mut sound, 0.55);
                room(&mut sound, 0.2);
            }
            Self::Incoming(Ringtone::Piano) => {
                for (at, chord) in [(0.0, [65, 69, 72, 76]), (0.6, [64, 67, 71, 74])] {
                    for (index, key) in chord.into_iter().enumerate() {
                        #[expect(clippy::cast_precision_loss, reason = "four notes")]
                        let strum = index as f32 * 0.018;
                        mix(
                            &mut sound,
                            at + strum,
                            &electric_piano(pitch(key), 0.7, 2.4),
                        );
                    }
                }
                for (at, key) in [(0.3, 79), (0.9, 81), (1.2, 79)] {
                    mix(&mut sound, at, &electric_piano(pitch(key), 0.9, 1.6));
                }
                room(&mut sound, 0.25);
            }
            // 425 Hz, the ringback tone of most of the world, for a second.
            // Two soft knocks of a low third on the electric piano: a ring
            // heard through the line, quieter than any ringtone.
            Self::Ringback => {
                for at in [0.0, 0.42] {
                    for key in [57, 61] {
                        mix(&mut sound, at, &electric_piano(pitch(key), 0.45, 1.8));
                    }
                }
                room(&mut sound, 0.18);
                return scaled(sound, 0.24);
            }
        }
        scaled(sound, PEAK)
    }

    /// Plays one round, unless the task is dropped first.
    pub(crate) fn play_once(self, cx: &App) -> Task<()> {
        let executor = cx.background_executor().clone();
        cx.spawn(async move |_| {
            let sound = executor.spawn(async move { self.samples() }).await;
            match Clip::play(&sound, RATE) {
                Ok(clip) => {
                    executor.timer(clip.length()).await;
                    drop(clip);
                }
                Err(error) => tracing::warn!(%error, "could not play the ringtone"),
            }
        })
    }

    /// Plays the sound over and over until the task is dropped.
    pub(crate) fn ring(self, cx: &App) -> Task<()> {
        let executor = cx.background_executor().clone();
        cx.spawn(async move |_| {
            let sound: Arc<[f32]> = executor.spawn(async move { self.samples() }).await.into();
            loop {
                let clip = Clip::play(&sound, RATE);
                if let Err(error) = &clip {
                    tracing::warn!(%error, "could not ring");
                }
                executor.timer(self.every()).await;
                drop(clip);
            }
        })
    }
}

/// A kalimba tine: its fundamental rings, the partial near six times it
/// gives the metal's bite, and the thumb's touch clicks.
fn kalimba(hertz: f32, loud: f32) -> Vec<f32> {
    let mut noise = Noise(3);
    (0..seconds(1.5))
        .map(|index| {
            let at = time(index);
            let tine = (TAU * hertz * at).sin() * (-at / 0.55).exp()
                + 0.35 * (TAU * hertz * 5.95 * at).sin() * (-at / 0.06).exp()
                + 0.12 * (TAU * hertz * 2.0 * at).sin() * (-at / 0.2).exp();
            let click = if at < 0.003 { 0.12 * noise.next() } else { 0.0 };
            loud * (tine * (at / 0.0015).min(1.0) + click)
        })
        .collect()
}

/// A plucked nylon string (Karplus-Strong): a burst of softened noise
/// running round a delay one period long, losing its highs as it goes.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a string's period is a few hundred samples"
)]
fn pluck(hertz: f32, loud: f32, seed: usize) -> Vec<f32> {
    let period = ((RATE_F32 / hertz) as usize).max(2);
    let mut noise = Noise(u64::try_from(seed).unwrap_or(0) + 1);
    let mut string: Vec<f32> = (0..period).map(|_| noise.next()).collect();
    soften(&mut string, 0.45);
    let mut at = 0;
    (0..seconds(2.2))
        .map(|_| {
            let sample = string[at];
            let next = string[(at + 1) % period];
            string[at] = 0.996 * 0.5 * (sample + next);
            at = (at + 1) % period;
            loud * sample
        })
        .collect()
}

/// An electric piano's tine and bar (FM, one to one), its bell fading
/// fast, with a slow tremolo.
fn electric_piano(hertz: f32, loud: f32, length: f32) -> Vec<f32> {
    (0..seconds(length))
        .map(|index| {
            let at = time(index);
            let depth = 1.2 * (-at / 0.25).exp() * loud;
            let body = (TAU * hertz * at + depth * (TAU * hertz * at).sin()).sin();
            let bell = 0.18 * (TAU * hertz * 7.0 * at).sin() * (-at / 0.05).exp();
            let fade = (-at / 1.2).exp() * (at / 0.004).min(1.0);
            let tremolo = 1.0 - 0.15 * (0.5 + 0.5 * (TAU * 4.5 * at).sin());
            loud * (body + bell) * fade * tremolo
        })
        .collect()
}

/// The frequency of MIDI key `key`.
fn pitch(key: i32) -> f32 {
    #[expect(clippy::cast_precision_loss, reason = "MIDI keys are small")]
    let above_a = (key - 69) as f32;
    440.0 * (above_a / 12.0).exp2()
}

/// Adds `note` into `sound`, starting `at` seconds in.
fn mix(sound: &mut [f32], at: f32, note: &[f32]) {
    for (slot, sample) in sound.iter_mut().skip(seconds(at)).zip(note) {
        *slot += sample;
    }
}

/// A gentle one-pole low-pass: `amount` near 1 keeps more of the highs.
fn soften(sound: &mut [f32], amount: f32) {
    let mut last = 0.0;
    for sample in sound {
        last += amount * (*sample - last);
        *sample = last;
    }
}

/// A small room (Schroeder: four damped combs, two all-passes), `wet` of
/// it mixed in.
fn room(sound: &mut [f32], wet: f32) {
    let mut echoes = vec![0.0; sound.len()];
    for (delay, feedback) in [(1557, 0.84), (1617, 0.83), (1491, 0.85), (1422, 0.84)] {
        let mut line = vec![0.0_f32; delay];
        let (mut at, mut damped) = (0, 0.0_f32);
        for (echo, sample) in echoes.iter_mut().zip(sound.iter()) {
            let out = line[at];
            damped = out * 0.6 + damped * 0.4;
            line[at] = sample + damped * feedback;
            at = (at + 1) % delay;
            *echo += out / 4.0;
        }
    }
    for delay in [225, 556] {
        let mut line = vec![0.0_f32; delay];
        let mut at = 0;
        for echo in &mut echoes {
            let held = line[at];
            let fed = *echo + held * 0.5;
            line[at] = fed;
            *echo = held - fed * 0.5;
            at = (at + 1) % delay;
        }
    }
    for (sample, echo) in sound.iter_mut().zip(echoes) {
        *sample = *sample * (1.0 - wet) + echo * wet;
    }
}

/// `sound`, peaking at `peak`.
fn scaled(mut sound: Vec<f32>, peak: f32) -> Vec<f32> {
    let loudest = sound
        .iter()
        .fold(0.0_f32, |most, sample| most.max(sample.abs()));
    if loudest > 0.0 {
        for sample in &mut sound {
            *sample *= peak / loudest;
        }
    }
    sound
}

/// White noise from a fixed seed, so every ring sounds the same.
struct Noise(u64);

impl Noise {
    #[expect(clippy::cast_precision_loss, reason = "24 bits of noise")]
    fn next(&mut self) -> f32 {
        // xorshift64
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 23) as f32 - 1.0
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a few seconds of samples"
)]
fn seconds(seconds: f32) -> usize {
    (seconds * RATE_F32) as usize
}

#[expect(clippy::cast_precision_loss, reason = "a few seconds of samples")]
fn time(index: usize) -> f32 {
    index as f32 / RATE_F32
}

#[cfg(test)]
mod tests {
    use super::{Ringing, Ringtone};

    fn every_sound() -> Vec<Ringing> {
        Ringtone::ALL
            .into_iter()
            .map(Ringing::Incoming)
            .chain([Ringing::Ringback])
            .collect()
    }

    #[test]
    fn each_sound_fills_its_round_without_clipping() {
        for sound in every_sound() {
            let samples = sound.samples();
            assert_eq!(
                samples.len(),
                super::seconds(sound.every().as_secs_f32()),
                "{sound:?}"
            );
            let loudest = samples.iter().fold(0.0_f32, |most, s| most.max(s.abs()));
            assert!(
                loudest > 0.1 && loudest < 1.0,
                "{sound:?} peaks at {loudest}"
            );
            assert!(samples.iter().all(|s| s.is_finite()), "{sound:?}");
        }
    }

    #[test]
    fn a_round_ends_quieter_than_it_rings() {
        // Each round dies away before the next starts, so rounds do not
        // pile up into a drone.
        for sound in every_sound() {
            let samples = sound.samples();
            let tail = &samples[samples.len() * 9 / 10..];
            let loudest_tail = tail.iter().fold(0.0_f32, |most, s| most.max(s.abs()));
            assert!(loudest_tail < 0.1, "{sound:?} still at {loudest_tail}");
        }
    }

    #[test]
    fn ringtones_read_back_from_their_keys() {
        for tone in Ringtone::ALL {
            assert_eq!(Ringtone::from_key(tone.key()), Some(tone));
        }
        assert_eq!(Ringtone::from_key("trumpet"), None);
    }
}
