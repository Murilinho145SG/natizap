//! Sample-rate conversion for live sound, which arrives in pieces: each
//! piece carries on from the last without a click, and going down in rate
//! filters out what the lower rate cannot hold instead of folding it back
//! as noise.

use std::f64::consts::PI;

/// Input samples weighed on each side of an output sample.
const HALF_TAPS: usize = 16;
/// Steps per input sample in the precomputed filter.
const PHASES: usize = 256;

/// Turns mono sound at one rate into the same sound at another.
pub(crate) struct Resampler {
    /// Input samples per output sample.
    step: f64,
    /// The filter's shape from distance 0 to [`HALF_TAPS`], [`PHASES`]
    /// steps per sample, plus one for the end.
    kernel: Vec<f32>,
    /// Input still needed by output samples to come.
    history: Vec<f32>,
    /// Where in `history` the next output sample falls.
    position: f64,
}

impl Resampler {
    /// Converts from `from` samples a second to `to`.
    pub(crate) fn new(from: u32, to: u32) -> Self {
        let step = f64::from(from) / f64::from(to.max(1));
        // Going down, keep only what fits under the new rate's Nyquist,
        // with a little room for the filter's slope.
        let cutoff = if step > 1.0 { 0.92 / step } else { 1.0 };
        #[expect(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            reason = "the kernel has a few thousand entries"
        )]
        let kernel = (0..=HALF_TAPS * PHASES)
            .map(|index| {
                let distance = index as f64 / PHASES as f64;
                let window = f64::midpoint(1.0, (PI * distance / HALF_TAPS as f64).cos());
                (cutoff * sinc(cutoff * distance) * window) as f32
            })
            .collect();
        Self {
            step,
            kernel,
            // Silence before the first sample, so it has neighbours too.
            history: vec![0.0; HALF_TAPS],
            #[expect(clippy::cast_precision_loss, reason = "a small constant")]
            position: HALF_TAPS as f64,
        }
    }

    /// Converts `input`, the samples that follow the last ones pushed,
    /// appending what is ready to `output`. The last few samples wait for
    /// the next piece, so the filter sees past them.
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "positions stay within a few pieces of sound"
    )]
    pub(crate) fn push(&mut self, input: &[f32], output: &mut Vec<f32>) {
        self.history.extend_from_slice(input);
        while (self.position.floor() as usize) + HALF_TAPS < self.history.len() {
            output.push(self.at(self.position));
            self.position += self.step;
        }
        let used = (self.position.floor() as usize).saturating_sub(HALF_TAPS);
        self.history.drain(..used);
        self.position -= used as f64;
    }

    /// The sound at `position` in `history`, from its neighbours.
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "positions stay within a few pieces of sound"
    )]
    fn at(&self, position: f64) -> f32 {
        let center = position.floor() as usize;
        let fraction = position - center as f64;
        (center + 1 - HALF_TAPS..=center + HALF_TAPS)
            .map(|index| {
                let distance = (index as f64 - center as f64 - fraction).abs();
                self.history[index] * self.weight(distance)
            })
            .sum()
    }

    /// The filter at `distance` samples, between its precomputed steps.
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "distances are within the filter's few samples"
    )]
    fn weight(&self, distance: f64) -> f32 {
        let at = distance * PHASES as f64;
        let index = at as usize;
        let Some(&before) = self.kernel.get(index) else {
            return 0.0;
        };
        let after = self.kernel.get(index + 1).copied().unwrap_or(0.0);
        let blend = (at - at.floor()) as f32;
        before + (after - before) * blend
    }
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-9 {
        1.0
    } else {
        (PI * x).sin() / (PI * x)
    }
}

#[cfg(test)]
mod tests {
    use super::Resampler;

    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "test signals are short"
    )]
    fn tone(hertz: f32, rate: u32, seconds: f32) -> Vec<f32> {
        let count = (rate as f32 * seconds) as usize;
        (0..count)
            .map(|index| (std::f32::consts::TAU * hertz * index as f32 / rate as f32).sin())
            .collect()
    }

    /// Loudness of `samples`, past the filter's start.
    #[expect(clippy::cast_precision_loss, reason = "test signals are short")]
    fn rms(samples: &[f32]) -> f32 {
        let steady = &samples[samples.len() / 4..];
        (steady.iter().map(|sample| sample * sample).sum::<f32>() / steady.len() as f32).sqrt()
    }

    fn convert(input: &[f32], from: u32, to: u32) -> Vec<f32> {
        let mut output = Vec::new();
        Resampler::new(from, to).push(input, &mut output);
        output
    }

    #[test]
    fn a_voice_keeps_its_loudness_going_down() {
        let output = convert(&tone(1_000.0, 48_000, 0.5), 48_000, 16_000);
        assert!((rms(&output) - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.03);
    }

    #[test]
    fn a_voice_keeps_its_loudness_going_up() {
        let output = convert(&tone(1_000.0, 16_000, 0.5), 16_000, 48_000);
        assert!((rms(&output) - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.03);
    }

    #[test]
    fn going_down_drops_what_the_lower_rate_cannot_hold() {
        // 12 kHz is over 16 kHz's Nyquist: kept, it would fold back to 4 kHz.
        let output = convert(&tone(12_000.0, 48_000, 0.5), 48_000, 16_000);
        assert!(rms(&output) < 0.02, "aliased at {}", rms(&output));
    }

    #[test]
    fn the_rate_changes_by_the_ratio() {
        let output = convert(&vec![0.0; 48_000], 48_000, 16_000);
        assert!(
            (15_980..=16_000).contains(&output.len()),
            "{}",
            output.len()
        );
    }

    #[test]
    fn pieces_join_as_if_pushed_at_once() {
        let input = tone(440.0, 44_100, 0.2);
        let whole = convert(&input, 44_100, 16_000);
        let mut resampler = Resampler::new(44_100, 16_000);
        let mut pieces = Vec::new();
        for piece in input.chunks(441) {
            resampler.push(piece, &mut pieces);
        }
        assert_eq!(whole.len(), pieces.len());
        for (whole, piece) in whole.iter().zip(&pieces) {
            assert!((whole - piece).abs() < 1e-5);
        }
    }
}
