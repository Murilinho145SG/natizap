//! The two sides of a call's sound in the sound card's callbacks: the
//! microphone's samples cleaned and cut into the call's frames, and the
//! call's frames played on every channel with a cushion against lateness.

use std::collections::VecDeque;
use std::sync::Arc;

use cpal::StreamConfig;

use super::{CallFormat, Meter};
use crate::resample::Resampler;
use crate::voice_processing::VoiceProcessing;

/// Sound held back before the speaker starts, and again after it ran
/// dry, so frames arriving a little late still play smoothly.
const CUSHION_SECONDS: f32 = 0.1;
/// The loudest sample of a frame of silence, give or take noise.
const VOICED: u16 = 300;
/// Past this much sound waiting, the oldest is dropped: a call that falls
/// behind catches up instead of answering ever later.
const MOST_SECONDS: f32 = 0.3;

/// The microphone's side: the card's samples, mono, at the call's rate,
/// cut into frames.
pub(super) struct Microphone {
    channels: usize,
    resampler: Resampler,
    frame: usize,
    /// Mono samples at the call's rate, not yet a whole piece to clean.
    raw: Vec<f32>,
    /// Cleaned samples, not yet a whole frame.
    pending: Vec<f32>,
    mono: Vec<f32>,
    heard: async_channel::Sender<Vec<i16>>,
    voice: VoiceProcessing,
    meter: Arc<Meter>,
}

impl Microphone {
    pub(super) fn new(
        config: StreamConfig,
        format: CallFormat,
        heard: async_channel::Sender<Vec<i16>>,
        voice: VoiceProcessing,
        meter: Arc<Meter>,
    ) -> Self {
        Self {
            channels: usize::from(config.channels).max(1),
            resampler: Resampler::new(config.sample_rate, format.rate),
            frame: format.frame,
            raw: Vec::with_capacity(format.frame),
            pending: Vec::with_capacity(format.frame * 2),
            mono: Vec::new(),
            heard,
            voice,
            meter,
        }
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "a sound card has a handful of channels"
    )]
    pub(super) fn hear(&mut self, input: &[f32]) {
        self.mono.clear();
        let channels = self.channels as f32;
        self.mono.extend(
            input
                .chunks(self.channels)
                .map(|frame| frame.iter().sum::<f32>() / channels),
        );
        self.resampler.push(&self.mono, &mut self.raw);
        let piece = self.voice.piece();
        let whole = self.raw.len() / piece * piece;
        for chunk in self.raw[..whole].chunks_exact_mut(piece) {
            self.voice.clean(chunk);
        }
        self.pending.extend(self.raw.drain(..whole));
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
pub(super) struct Speaker {
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
    voice: VoiceProcessing,
    meter: Arc<Meter>,
}

impl Speaker {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "a fraction of a second of samples"
    )]
    pub(super) fn new(
        config: StreamConfig,
        format: CallFormat,
        said: async_channel::Receiver<Vec<i16>>,
        voice: VoiceProcessing,
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
            voice,
            meter,
        }
    }

    pub(super) fn fill(&mut self, output: &mut [f32]) {
        while let Ok(frame) = self.said.try_recv() {
            Meter::add(&self.meter.received, 1);
            if frame.iter().any(|sample| sample.unsigned_abs() > VOICED) {
                Meter::add(&self.meter.voiced, 1);
            }
            self.converted.clear();
            let samples: Vec<f32> = frame.iter().copied().map(to_f32).collect();
            // What plays now comes back through the microphone a moment
            // later: the cleaning finds it there by this.
            self.voice.played(&samples);
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
pub(super) fn to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
}

fn to_f32(sample: i16) -> f32 {
    f32::from(sample) / f32::from(i16::MAX)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use cpal::StreamConfig;

    use super::{CallFormat, Microphone, Speaker, VoiceProcessing};

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
        let mut microphone = Microphone::new(
            card(48_000, 2),
            FORMAT,
            heard,
            VoiceProcessing::new(FORMAT.rate),
            Arc::default(),
        );
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
        let mut speaker = Speaker::new(
            card(16_000, 2),
            FORMAT,
            frames,
            VoiceProcessing::new(FORMAT.rate),
            Arc::default(),
        );
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
        let mut speaker = Speaker::new(
            card(16_000, 1),
            FORMAT,
            frames,
            VoiceProcessing::new(FORMAT.rate),
            Arc::default(),
        );
        for _ in 0..20 {
            said.try_send(vec![0; 960]).ok();
        }
        speaker.fill(&mut [0.0; 16]);
        assert!(speaker.queue.len() <= speaker.cushion);
    }
}
