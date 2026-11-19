//! Sound for a call: the microphone and the speaker, in the rate and the
//! frames a call's codec takes, whatever the sound card's own.
//!
//! Everything happens in the sound card's callbacks, which never wait:
//! frames go to the call and come from it through channels tried, never
//! awaited, so a slow call drops a frame instead of stalling the card.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use cpal::traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _};
use cpal::{Stream, StreamConfig};

use crate::audio::closest_f32;
use crate::opus::OpusDecoder;
use crate::resample::Resampler;

/// Frames waiting for the call before new ones are dropped.
const MICROPHONE_BACKLOG: usize = 4;
/// Frames from the call waiting for the speaker's callback.
const SPEAKER_BACKLOG: usize = 8;
/// Sound held back before the speaker starts, and again after it ran
/// dry, so frames arriving a little late still play smoothly.
const CUSHION_SECONDS: f32 = 0.1;
/// Past this much sound waiting, the oldest is dropped: a call that falls
/// behind catches up instead of answering ever later.
const MOST_SECONDS: f32 = 0.3;

/// Why a call's sound could not start.
#[derive(Debug, thiserror::Error)]
pub enum CallAudioError {
    /// There is no microphone.
    #[error("no microphone found")]
    NoMicrophone,
    /// There is no speaker or headset.
    #[error("no sound output device")]
    NoSpeaker,
    /// A device refused to open.
    #[error("the sound device failed: {0}")]
    Device(String),
}

/// What a call's sound is like: mono, `rate` samples a second, in frames of
/// `frame` samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallFormat {
    /// Samples a second.
    pub rate: u32,
    /// Samples in each frame sent or received.
    pub frame: usize,
}

/// The ends of a call's sound, for the call to take.
#[derive(Debug, Clone)]
pub struct CallPorts {
    /// Frames heard by the microphone.
    pub microphone: async_channel::Receiver<Vec<i16>>,
    /// Frames for the speaker to play.
    pub speaker: async_channel::Sender<Vec<i16>>,
    /// Opus packets for the speaker to play, decoded on a thread of their
    /// own: the other end may send Opus where the call expects its own
    /// codec.
    pub opus: async_channel::Sender<Vec<u8>>,
}

/// How much sound went each way, told in the log when a call's sound
/// closes: where a call's voice got lost is otherwise invisible.
#[derive(Debug, Default)]
struct Meter {
    /// Frames the microphone sent to the call.
    sent: AtomicU64,
    /// Frames lost because the call was behind.
    unsent: AtomicU64,
    /// Frames the call gave the speaker, Opus ones included.
    received: AtomicU64,
    /// Opus packets decoded.
    opus: AtomicU64,
    /// Samples played that were not silence filling a gap.
    played: AtomicU64,
}

impl Meter {
    fn add(counter: &AtomicU64, count: usize) {
        counter.fetch_add(u64::try_from(count).unwrap_or(u64::MAX), Ordering::Relaxed);
    }
}

/// The microphone and speaker open for a call. Dropping it closes both.
pub struct CallAudio {
    _microphone: Stream,
    _speaker: Stream,
    meter: Arc<Meter>,
}

impl Drop for CallAudio {
    fn drop(&mut self) {
        let read = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        let meter = &self.meter;
        tracing::info!(
            sent = read(&meter.sent),
            unsent = read(&meter.unsent),
            received = read(&meter.received),
            opus = read(&meter.opus),
            played = read(&meter.played),
            "call sound closed: frames sent and received, Opus packets decoded, samples played"
        );
    }
}

impl std::fmt::Debug for CallAudio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallAudio").finish_non_exhaustive()
    }
}

impl CallAudio {
    /// Opens the default microphone and speaker for a call in `format`.
    ///
    /// # Errors
    ///
    /// Fails when either device is missing or refuses to open.
    pub fn open(format: CallFormat) -> Result<(Self, CallPorts), CallAudioError> {
        let host = cpal::default_host();
        let (heard, microphone) = async_channel::bounded(MICROPHONE_BACKLOG);
        let (speaker, said) = async_channel::bounded(SPEAKER_BACKLOG);
        let input = host
            .default_input_device()
            .ok_or(CallAudioError::NoMicrophone)?;
        let output = host
            .default_output_device()
            .ok_or(CallAudioError::NoSpeaker)?;
        let meter = Arc::new(Meter::default());
        let calls = Self {
            _microphone: listen(&input, format, heard, Arc::clone(&meter))?,
            _speaker: play(&output, format, said, Arc::clone(&meter))?,
            meter: Arc::clone(&meter),
        };
        let (opus, packets) = async_channel::bounded(SPEAKER_BACKLOG);
        let decoded = speaker.clone();
        // A plain thread: FFmpeg blocks, and the card's callbacks must not.
        // It ends once the call drops its end of `opus`.
        std::thread::Builder::new()
            .name("natizap-call-opus".into())
            .spawn(move || decode_opus(&packets, &decoded, format, &meter))
            .map_err(device_error)?;
        Ok((
            calls,
            CallPorts {
                microphone,
                speaker,
                opus,
            },
        ))
    }
}

fn listen(
    device: &cpal::Device,
    format: CallFormat,
    heard: async_channel::Sender<Vec<i16>>,
    meter: Arc<Meter>,
) -> Result<Stream, CallAudioError> {
    let config = closest_f32(
        &device.default_input_config().map_err(device_error)?,
        device.supported_input_configs().map_err(device_error)?,
    )
    .ok_or_else(|| CallAudioError::Device("the microphone gives no f32 samples".into()))?;
    tracing::info!(
        rate = config.sample_rate,
        channels = config.channels,
        "call microphone open"
    );
    let mut microphone = Microphone::new(config, format, heard, meter);
    let stream = device
        .build_input_stream(
            config,
            move |input: &[f32], _| microphone.hear(input),
            |error| tracing::warn!(%error, "microphone failed during a call"),
            None,
        )
        .map_err(device_error)?;
    stream.play().map_err(device_error)?;
    Ok(stream)
}

fn play(
    device: &cpal::Device,
    format: CallFormat,
    said: async_channel::Receiver<Vec<i16>>,
    meter: Arc<Meter>,
) -> Result<Stream, CallAudioError> {
    let config = closest_f32(
        &device.default_output_config().map_err(device_error)?,
        device.supported_output_configs().map_err(device_error)?,
    )
    .ok_or_else(|| CallAudioError::Device("the speaker takes no f32 samples".into()))?;
    tracing::info!(
        rate = config.sample_rate,
        channels = config.channels,
        "call speaker open"
    );
    let mut speaker = Speaker::new(config, format, said, meter);
    let stream = device
        .build_output_stream(
            config,
            move |output: &mut [f32], _| speaker.fill(output),
            |error| tracing::warn!(%error, "speaker failed during a call"),
            None,
        )
        .map_err(device_error)?;
    stream.play().map_err(device_error)?;
    Ok(stream)
}

/// Plays `packets` through `speaker`, at the call's rate.
fn decode_opus(
    packets: &async_channel::Receiver<Vec<u8>>,
    speaker: &async_channel::Sender<Vec<i16>>,
    format: CallFormat,
    meter: &Meter,
) {
    let mut decoder = match OpusDecoder::new() {
        Ok(decoder) => decoder,
        Err(error) => {
            tracing::warn!(%error, "no Opus decoder: Opus from the other end stays silent");
            return;
        }
    };
    let mut resampler: Option<(u32, Resampler)> = None;
    let (mut sound, mut converted) = (Vec::new(), Vec::new());
    while let Ok(packet) = packets.recv_blocking() {
        sound.clear();
        let rate = match decoder.decode(&packet, &mut sound) {
            Ok(Some(rate)) => rate,
            Ok(None) => continue,
            Err(error) => {
                // A damaged packet is a moment of silence, as on a bad line.
                tracing::debug!(%error, "dropped an Opus packet");
                continue;
            }
        };
        let resampler = match &mut resampler {
            Some((from, resampler)) if *from == rate => resampler,
            slot => &mut slot.insert((rate, Resampler::new(rate, format.rate))).1,
        };
        Meter::add(&meter.opus, 1);
        converted.clear();
        resampler.push(&sound, &mut converted);
        // Full means the speaker is behind: this piece is lost.
        let _ = speaker.try_send(converted.iter().copied().map(to_i16).collect());
    }
}

fn device_error(error: impl std::fmt::Display) -> CallAudioError {
    CallAudioError::Device(error.to_string())
}

/// The microphone's side: the card's samples, mono, at the call's rate,
/// cut into frames.
struct Microphone {
    channels: usize,
    resampler: Resampler,
    frame: usize,
    /// Mono samples at the call's rate, not yet a whole frame.
    pending: Vec<f32>,
    mono: Vec<f32>,
    heard: async_channel::Sender<Vec<i16>>,
    meter: Arc<Meter>,
}

impl Microphone {
    fn new(
        config: StreamConfig,
        format: CallFormat,
        heard: async_channel::Sender<Vec<i16>>,
        meter: Arc<Meter>,
    ) -> Self {
        Self {
            channels: usize::from(config.channels).max(1),
            resampler: Resampler::new(config.sample_rate, format.rate),
            frame: format.frame,
            pending: Vec::with_capacity(format.frame * 2),
            mono: Vec::new(),
            heard,
            meter,
        }
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "a sound card has a handful of channels"
    )]
    fn hear(&mut self, input: &[f32]) {
        self.mono.clear();
        let channels = self.channels as f32;
        self.mono.extend(
            input
                .chunks(self.channels)
                .map(|frame| frame.iter().sum::<f32>() / channels),
        );
        self.resampler.push(&self.mono, &mut self.pending);
        while self.pending.len() >= self.frame {
            let frame: Vec<i16> = self.pending.drain(..self.frame).map(to_i16).collect();
            // A full channel means the call is behind: this frame is lost,
            // as it would be on a bad network.
            let counter = if self.heard.try_send(frame).is_ok() {
                &self.meter.sent
            } else {
                &self.meter.unsent
            };
            Meter::add(counter, 1);
        }
    }
}

/// The speaker's side: the call's frames, at the card's rate, on every
/// channel, with a cushion against frames that come late.
struct Speaker {
    channels: usize,
    resampler: Resampler,
    said: async_channel::Receiver<Vec<i16>>,
    /// Mono samples at the card's rate, waiting to play.
    queue: VecDeque<f32>,
    converted: Vec<f32>,
    cushion: usize,
    most: usize,
    /// Whether the cushion is full and sound is playing.
    playing: bool,
    meter: Arc<Meter>,
}

impl Speaker {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "a fraction of a second of samples"
    )]
    fn new(
        config: StreamConfig,
        format: CallFormat,
        said: async_channel::Receiver<Vec<i16>>,
        meter: Arc<Meter>,
    ) -> Self {
        let rate = config.sample_rate as f32;
        Self {
            channels: usize::from(config.channels).max(1),
            resampler: Resampler::new(format.rate, config.sample_rate),
            said,
            queue: VecDeque::new(),
            converted: Vec::new(),
            cushion: (rate * CUSHION_SECONDS) as usize,
            most: (rate * MOST_SECONDS) as usize,
            playing: false,
            meter,
        }
    }

    fn fill(&mut self, output: &mut [f32]) {
        while let Ok(frame) = self.said.try_recv() {
            Meter::add(&self.meter.received, 1);
            self.converted.clear();
            let samples: Vec<f32> = frame.iter().copied().map(to_f32).collect();
            self.resampler.push(&samples, &mut self.converted);
            self.queue.extend(&self.converted);
        }
        if self.queue.len() > self.most {
            let late = self.queue.len() - self.cushion;
            self.queue.drain(..late);
        }
        if !self.playing && self.queue.len() >= self.cushion {
            self.playing = true;
        }
        for frame in output.chunks_mut(self.channels) {
            let sample = if self.playing {
                self.queue.pop_front()
            } else {
                None
            };
            frame.fill(sample.unwrap_or(0.0));
            if sample.is_none() {
                self.playing = false;
            } else {
                Meter::add(&self.meter.played, 1);
            }
        }
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "clamped to i16's range first"
)]
fn to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
}

fn to_f32(sample: i16) -> f32 {
    f32::from(sample) / f32::from(i16::MAX)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use cpal::StreamConfig;

    use super::{CallFormat, Microphone, Speaker};

    const FORMAT: CallFormat = CallFormat {
        rate: 16_000,
        frame: 960,
    };

    fn card(rate: u32, channels: u16) -> StreamConfig {
        StreamConfig {
            channels,
            sample_rate: rate,
            buffer_size: cpal::BufferSize::Default,
        }
    }

    #[test]
    fn the_microphone_sends_whole_frames_at_the_calls_rate() {
        let (heard, frames) = async_channel::unbounded();
        let mut microphone = Microphone::new(card(48_000, 2), FORMAT, heard, Arc::default());
        // One second of stereo sound at 48 kHz, in the card's small pieces.
        for _ in 0..100 {
            microphone.hear(&[0.5; 960]);
        }
        let mut count = 0;
        while let Ok(frame) = frames.try_recv() {
            assert_eq!(frame.len(), 960);
            count += 1;
        }
        // 16 000 samples make 16 frames, less the filter's lead.
        assert_eq!(count, 16);
    }

    #[test]
    fn the_speaker_waits_for_its_cushion_then_plays_on_every_channel() {
        let (said, frames) = async_channel::unbounded();
        let mut speaker = Speaker::new(card(16_000, 2), FORMAT, frames, Arc::default());
        let mut output = vec![1.0; 320];
        said.try_send(vec![i16::MAX / 2; 960]).ok();
        speaker.fill(&mut output);
        assert!(output.iter().all(|sample| *sample == 0.0), "played early");
        said.try_send(vec![i16::MAX / 2; 960]).ok();
        speaker.fill(&mut output);
        // Past the filter's lead, both channels carry the voice.
        let (left, right) = (output[300], output[301]);
        assert!((left - 0.5).abs() < 0.01, "{left}");
        assert!((left - right).abs() < f32::EPSILON);
    }

    #[test]
    fn a_speaker_that_falls_behind_drops_the_oldest_sound() {
        let (said, frames) = async_channel::unbounded();
        let mut speaker = Speaker::new(card(16_000, 1), FORMAT, frames, Arc::default());
        for _ in 0..20 {
            said.try_send(vec![0; 960]).ok();
        }
        speaker.fill(&mut [0.0; 16]);
        assert!(speaker.queue.len() <= speaker.cushion);
    }
}
