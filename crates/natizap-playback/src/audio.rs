use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use cpal::traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _};
use cpal::{
    BufferSize, FromSample, SampleFormat, SizedSample, Stream, StreamConfig, SupportedBufferSize,
    SupportedStreamConfig, SupportedStreamConfigRange,
};

use crate::PlaybackError;
use crate::clock::Clock;

/// How much sound the card takes at a time when playing media: enough to
/// ride out a busy moment, little enough to keep sound with the pictures.
const MEDIA_PERIOD: Duration = Duration::from_millis(40);

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
    closest_f32(&preferred, supported, MEDIA_PERIOD)
        .ok_or_else(|| PlaybackError::Audio("the sound card does not take f32 samples".into()))
}

/// `preferred` if it is in `f32` samples, else the first of `supported`
/// that is, at the rate closest to `preferred`'s; with the card asked to
/// take sound `period` at a time, as near as it allows.
///
/// Left to choose, a sound server buffers as much as it likes: `PulseAudio`,
/// and `PipeWire` speaking its protocol, hold seconds, which a call hears as
/// delay and a video as sound behind its pictures.
pub(crate) fn closest_f32(
    preferred: &SupportedStreamConfig,
    mut supported: impl Iterator<Item = SupportedStreamConfigRange>,
    period: Duration,
) -> Option<StreamConfig> {
    let chosen = if preferred.sample_format() == SampleFormat::F32 {
        *preferred
    } else {
        let range = supported.find(|range| range.sample_format() == SampleFormat::F32)?;
        let rate = preferred
            .sample_rate()
            .clamp(range.min_sample_rate(), range.max_sample_rate());
        range.with_sample_rate(rate)
    };
    Some(paced(&chosen, period))
}

/// `chosen`, with the card asked to take sound `period` at a time.
fn paced(chosen: &SupportedStreamConfig, period: Duration) -> StreamConfig {
    let mut config = chosen.config();
    if let SupportedBufferSize::Range { min, max } = *chosen.buffer_size() {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a few milliseconds of frames"
        )]
        let frames = (f64::from(config.sample_rate) * period.as_secs_f64()) as u32;
        config.buffer_size = BufferSize::Fixed(frames.clamp(min, max));
    }
    config
}

/// Opens `device`'s microphone `period` at a time, handing what it hears
/// as `f32` samples, whatever it gives them in, to what `listener` makes
/// for the configuration chosen: some microphones give only integers
/// (Windows' often give 16-bit ones), and refusing them would leave the
/// user unheard.
pub(crate) fn open_microphone<H>(
    device: &cpal::Device,
    period: Duration,
    listener: impl FnOnce(&StreamConfig) -> H,
    failed: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<(Stream, StreamConfig), String>
where
    H: FnMut(&[f32]) + Send + 'static,
{
    let preferred = device
        .default_input_config()
        .map_err(|error| error.to_string())?;
    let supported = device
        .supported_input_configs()
        .map_err(|error| error.to_string())?;
    let (config, format) = match closest_f32(&preferred, supported, period) {
        Some(config) => (config, SampleFormat::F32),
        None => (paced(&preferred, period), preferred.sample_format()),
    };
    let hear = listener(&config);
    let stream = match format {
        SampleFormat::F32 => listen::<f32>(device, &config, hear, failed),
        SampleFormat::F64 => listen::<f64>(device, &config, hear, failed),
        SampleFormat::I8 => listen::<i8>(device, &config, hear, failed),
        SampleFormat::I16 => listen::<i16>(device, &config, hear, failed),
        SampleFormat::I32 => listen::<i32>(device, &config, hear, failed),
        SampleFormat::U8 => listen::<u8>(device, &config, hear, failed),
        SampleFormat::U16 => listen::<u16>(device, &config, hear, failed),
        SampleFormat::U32 => listen::<u32>(device, &config, hear, failed),
        other => return Err(format!("the microphone gives {other} samples")),
    }
    .map_err(|error| error.to_string())?;
    Ok((stream, config))
}

/// The microphone's stream in `T` samples, handed on as `f32`.
fn listen<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut hear: impl FnMut(&[f32]) + Send + 'static,
    failed: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<Stream, cpal::Error>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let mut heard: Vec<f32> = Vec::new();
    device.build_input_stream(
        *config,
        move |input: &[T], _| {
            heard.clear();
            heard.extend(input.iter().map(|sample| sample.to_sample::<f32>()));
            hear(&heard);
        },
        failed,
        None,
    )
}
