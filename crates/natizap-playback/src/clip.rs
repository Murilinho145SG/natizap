//! Short sounds held in memory, such as a notification's, played straight
//! to the sound card without a file or a decoder.

use std::sync::Arc;
use std::time::Duration;

use crate::PlaybackError;
use crate::audio::AudioOutput;

/// Why a clip cannot be played.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ClipError(#[from] PlaybackError);

/// A clip playing. It stops when dropped, so keep it for its
/// [`Clip::length`].
pub struct Clip {
    _output: AudioOutput,
    length: Duration,
}

impl std::fmt::Debug for Clip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Clip")
            .field("length", &self.length)
            .finish_non_exhaustive()
    }
}

impl Clip {
    /// Plays mono `samples`, `rate` per second, on the default output.
    ///
    /// # Errors
    ///
    /// Fails when there is no sound card or it refuses the stream.
    #[expect(
        clippy::cast_precision_loss,
        reason = "clips are seconds long, far inside f64's exact range"
    )]
    pub fn play(samples: &[f32], rate: u32) -> Result<Self, ClipError> {
        let output = AudioOutput::open(Arc::default())?;
        let resampled = resample(samples, rate, output.rate);
        let channels = usize::from(output.channels);
        let interleaved: Vec<f32> = resampled
            .iter()
            .flat_map(|&sample| std::iter::repeat_n(sample, channels))
            .collect();
        output.push(&interleaved);
        let length = Duration::from_secs_f64(samples.len() as f64 / f64::from(rate.max(1)));
        Ok(Self {
            _output: output,
            length,
        })
    }

    /// How long the clip plays.
    #[must_use]
    pub fn length(&self) -> Duration {
        self.length
    }
}

/// `samples` at `from` per second, linearly resampled to `to` per second.
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clips are seconds long, far inside f64's exact range"
)]
fn resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || samples.is_empty() || from == 0 {
        return samples.to_vec();
    }
    let step = f64::from(from) / f64::from(to);
    let count = (samples.len() as f64 / step) as usize;
    (0..count)
        .map(|index| {
            let at = index as f64 * step;
            let before = at as usize;
            let after = (before + 1).min(samples.len() - 1);
            let weight = (at - before as f64) as f32;
            samples[before] * (1.0 - weight) + samples[after] * weight
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::resample;

    #[test]
    fn keeps_samples_at_the_same_rate() {
        assert_eq!(resample(&[0.1, 0.2], 48_000, 48_000), vec![0.1, 0.2]);
    }

    #[test]
    fn doubles_samples_at_twice_the_rate() {
        let resampled = resample(&[0.0, 1.0, 0.0, -1.0], 24_000, 48_000);
        assert_eq!(resampled.len(), 8);
        assert!((resampled[1] - 0.5).abs() < 1e-6);
        assert!((resampled[2] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn halves_samples_at_half_the_rate() {
        let resampled = resample(&[0.0, 1.0, 2.0, 3.0], 48_000, 24_000);
        assert_eq!(resampled, vec![0.0, 2.0]);
    }
}
