//! Cleaning a call's microphone before it is sent: the other end's voice
//! coming back from the speaker is cancelled, steady background noise is
//! lowered and the level is kept even. It is WebRTC's own audio processing,
//! ported to Rust, so a call through laptop speakers sounds as it does in a
//! browser call.

use std::sync::{Arc, Mutex, PoisonError};

use sonora::config::{EchoCanceller, GainController2, HighPassFilter, NoiseSuppression};
use sonora::{AudioProcessing, Config, StreamConfig};

/// What the speaker plays and what the microphone hears, both mono at one
/// rate, processed in the 10 ms pieces the processing works in. Clones share
/// one processor: the speaker's side and the microphone's run on different
/// threads.
#[derive(Clone)]
pub(crate) struct VoiceProcessing {
    processor: Arc<Mutex<AudioProcessing>>,
    piece: usize,
}

impl VoiceProcessing {
    /// Processing for sound at `rate` samples a second.
    pub(crate) fn new(rate: u32) -> Self {
        let config = Config {
            high_pass_filter: Some(HighPassFilter::default()),
            echo_canceller: Some(EchoCanceller::default()),
            noise_suppression: Some(NoiseSuppression::default()),
            gain_controller2: Some(GainController2::default()),
            ..Config::default()
        };
        let stream = StreamConfig::new(rate, 1);
        let processor = AudioProcessing::builder()
            .config(config)
            .capture_config(stream)
            .render_config(stream)
            .build();
        Self {
            processor: Arc::new(Mutex::new(processor)),
            piece: (rate / 100) as usize,
        }
    }

    /// Samples in each piece the processing works in.
    pub(crate) fn piece(&self) -> usize {
        self.piece
    }

    /// Tells the processing what the speaker is about to play, so its echo
    /// can be found in what the microphone hears. Pieces short of a whole
    /// one at the end are dropped: the call's frames are whole pieces.
    pub(crate) fn played(&self, sound: &[f32]) {
        let mut processor = self.lock();
        let mut out = vec![0.0; self.piece];
        for piece in sound.chunks_exact(self.piece) {
            if let Err(error) = processor.process_render_f32(&[piece], &mut [&mut out]) {
                tracing::debug!(?error, "could not hear what the speaker plays");
            }
        }
    }

    /// Cleans `piece`, one whole piece the microphone heard, in place.
    pub(crate) fn clean(&self, piece: &mut [f32]) {
        let heard = piece.to_vec();
        if let Err(error) = self.lock().process_capture_f32(&[&heard], &mut [piece]) {
            tracing::debug!(?error, "could not clean the microphone");
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, AudioProcessing> {
        // The processor's state is a filter's: a panic mid-piece leaves it
        // as usable as a dropped piece would.
        self.processor
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::VoiceProcessing;

    /// Speech-like noise, the same every run.
    fn voice(samples: usize) -> Vec<f32> {
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        #[expect(clippy::cast_precision_loss, reason = "noise samples")]
        (0..samples)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                ((state >> 40) as f32 / (1u64 << 23) as f32 - 1.0) * 0.3
            })
            .collect()
    }

    fn rms(samples: &[f32]) -> f32 {
        #[expect(clippy::cast_precision_loss, reason = "short test signals")]
        let count = samples.len() as f32;
        (samples.iter().map(|s| s * s).sum::<f32>() / count).sqrt()
    }

    #[test]
    fn the_other_ends_voice_coming_back_through_the_speaker_is_cancelled() {
        let processing = VoiceProcessing::new(16_000);
        let piece = processing.piece();
        let far = voice(16_000 * 4);
        // The room sends the speaker back to the microphone 40 ms late and
        // quieter, as laptop speakers do.
        let delay = 640;
        let mut cleaned = Vec::new();
        let mut heard_total = Vec::new();
        for (index, played) in far.chunks_exact(piece).enumerate() {
            processing.played(played);
            let start = index * piece;
            let mut heard: Vec<f32> = (start..start + piece)
                .map(|at| at.checked_sub(delay).map_or(0.0, |at| far[at] * 0.5))
                .collect();
            heard_total.extend_from_slice(&heard);
            processing.clean(&mut heard);
            cleaned.extend_from_slice(&heard);
        }
        // Once it has found the echo, little of it is left.
        let last = cleaned.len() - 16_000;
        let (left, came_back) = (rms(&cleaned[last..]), rms(&heard_total[last..]));
        assert!(left < came_back * 0.2, "{left} of {came_back} left");
    }
}
