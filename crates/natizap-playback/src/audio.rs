use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use cpal::traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _};
use cpal::{SampleFormat, Stream, StreamConfig, SupportedStreamConfig, SupportedStreamConfigRange};

use crate::PlaybackError;
use crate::clock::Clock;

/// The sound card output: decoded samples queue here and the card's callback
/// plays them, advancing the clock by what it actually played.
pub(crate) struct AudioOutput {
    stream: Stream,
    queue: Arc<Mutex<VecDeque<f32>>>,
    pub(crate) rate: u32,
    pub(crate) channels: u16,
}

impl AudioOutput {
    /// Opens the default output device for interleaved `f32` samples.
    pub(crate) fn open(clock: Arc<Clock>) -> Result<Self, PlaybackError> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or(PlaybackError::NoAudioDevice)?;
        let config = f32_config(&device)?;
        let channels = usize::from(config.channels);
        clock.drive_with_audio(config.sample_rate);

        let queue: Arc<Mutex<VecDeque<f32>>> = Arc::default();
        let playing = Arc::clone(&queue);
        let stream = device
            .build_output_stream(
                config,
                move |out: &mut [f32], _| {
                    let mut queue = playing.lock().unwrap_or_else(PoisonError::into_inner);
                    let available = queue.len().min(out.len());
                    for (slot, sample) in out.iter_mut().zip(queue.drain(..available)) {
                        *slot = sample;
                    }
                    // Underruns play silence and do not advance the clock, so
                    // video waits for sound instead of running ahead.
                    out[available..].fill(0.0);
                    clock.advance((available / channels) as u64);
                },
                |error| tracing::warn!(%error, "audio output failed"),
                None,
            )
            .map_err(|error| PlaybackError::Audio(error.to_string()))?;
        stream
            .play()
            .map_err(|error| PlaybackError::Audio(error.to_string()))?;
        Ok(Self {
            stream,
            queue,
            rate: config.sample_rate,
            channels: config.channels,
        })
    }

    /// Queues interleaved samples in the output's format.
    pub(crate) fn push(&self, samples: &[f32]) {
        self.lock().extend(samples);
    }

    /// How much sound is queued and not yet played, in seconds.
    #[expect(
        clippy::cast_precision_loss,
        reason = "queue lengths are far below f32's exact range"
    )]
    pub(crate) fn queued_seconds(&self) -> f32 {
        self.lock().len() as f32 / (self.rate as f32 * f32::from(self.channels))
    }

    /// Drops everything queued, e.g. after a seek.
    pub(crate) fn clear(&self) {
        self.lock().clear();
    }

    pub(crate) fn set_paused(&self, paused: bool) {
        let result = if paused {
            self.stream.pause()
        } else {
            self.stream.play()
        };
        if let Err(error) = result {
            tracing::warn!(%error, "could not pause or resume audio");
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<f32>> {
        // The queue stays valid even if a holder panicked mid-update.
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The device's preferred configuration, in `f32` samples.
fn f32_config(device: &cpal::Device) -> Result<StreamConfig, PlaybackError> {
    let preferred = device
        .default_output_config()
        .map_err(|error| PlaybackError::Audio(error.to_string()))?;
    let supported = device
        .supported_output_configs()
        .map_err(|error| PlaybackError::Audio(error.to_string()))?;
    closest_f32(&preferred, supported)
        .ok_or_else(|| PlaybackError::Audio("the sound card does not take f32 samples".into()))
}

/// `preferred` if it is in `f32` samples, else the first of `supported`
/// that is, at the rate closest to `preferred`'s.
pub(crate) fn closest_f32(
    preferred: &SupportedStreamConfig,
    mut supported: impl Iterator<Item = SupportedStreamConfigRange>,
) -> Option<StreamConfig> {
    if preferred.sample_format() == SampleFormat::F32 {
        return Some(preferred.config());
    }
    supported
        .find(|range| range.sample_format() == SampleFormat::F32)
        .map(|range| {
            let rate = preferred
                .sample_rate()
                .clamp(range.min_sample_rate(), range.max_sample_rate());
            range.with_sample_rate(rate).config()
        })
}
