//! Recording a voice note: the microphone through `cpal`, encoded as Opus
//! in an Ogg file, which is what WhatsApp sends voice notes as.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use cpal::Stream;
use cpal::traits::{HostTrait as _, StreamTrait as _};
use ffmpeg::util::frame;
use ffmpeg::{ChannelLayout, Dictionary, codec, format};
use ffmpeg_next as ffmpeg;

use crate::audio::open_microphone;

/// How much the microphone hands over at a time.
const PERIOD: Duration = Duration::from_millis(40);
/// Opus's own rate, which WhatsApp's voice notes use.
const RATE: u32 = 48_000;
/// [`RATE`] as FFmpeg takes it.
const RATE_I32: i32 = 48_000;
/// Bars in a voice note's waveform, as the phone sends it.
const BARS: usize = 64;

/// Why recording failed.
#[derive(Debug, thiserror::Error)]
pub enum RecordError {
    /// There is no microphone.
    #[error("no microphone found")]
    NoMicrophone,
    /// The microphone could not be opened or read.
    #[error("the microphone failed: {0}")]
    Microphone(String),
    /// The recording could not be encoded or written.
    #[error("could not save the recording: {0}")]
    Encode(#[from] ffmpeg::Error),
    /// Nothing was recorded.
    #[error("nothing was recorded")]
    Empty,
}

/// A voice note being recorded. Dropping it throws the recording away.
pub struct Recorder {
    _stream: Stream,
    /// What was heard, the channels interleaved.
    samples: Arc<Mutex<Vec<f32>>>,
    channels: usize,
    rate: u32,
    started: Instant,
}

impl std::fmt::Debug for Recorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recorder")
            .field("rate", &self.rate)
            .finish_non_exhaustive()
    }
}

/// A finished voice note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recording {
    /// How long it plays.
    pub seconds: u32,
    /// Loudness over its length: 64 values, 0 to 100.
    pub waveform: Vec<u8>,
}

impl Recorder {
    /// Starts recording from the default microphone.
    ///
    /// # Errors
    ///
    /// Fails when there is no microphone or it cannot be opened.
    pub fn start() -> Result<Self, RecordError> {
        let device = cpal::default_host()
            .default_input_device()
            .ok_or(RecordError::NoMicrophone)?;
        let samples: Arc<Mutex<Vec<f32>>> = Arc::default();
        let heard = Arc::clone(&samples);
        let (stream, config) = open_microphone(
            &device,
            PERIOD,
            |_| {
                move |input: &[f32]| {
                    let mut heard = heard.lock().unwrap_or_else(PoisonError::into_inner);
                    heard.extend_from_slice(input);
                }
            },
            |error| tracing::warn!(%error, "microphone failed"),
        )
        .map_err(RecordError::Microphone)?;
        stream
            .play()
            .map_err(|error| RecordError::Microphone(error.to_string()))?;
        Ok(Self {
            _stream: stream,
            samples,
            channels: usize::from(config.channels).max(1),
            rate: config.sample_rate,
            started: Instant::now(),
        })
    }

    /// How long it has been recording.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Stops recording and hands back what was heard, to [`save`] off the
    /// UI thread.
    #[must_use]
    pub fn stop(self) -> Heard {
        let interleaved =
            std::mem::take(&mut *self.samples.lock().unwrap_or_else(PoisonError::into_inner));
        // Voice notes are mono: average the channels.
        #[expect(
            clippy::cast_precision_loss,
            reason = "a sound card has a handful of channels"
        )]
        let samples = interleaved
            .chunks(self.channels)
            .map(|frame| frame.iter().sum::<f32>() / self.channels as f32)
            .collect();
        Heard {
            samples,
            rate: self.rate,
        }
    }
}

/// What the microphone heard, mono, at the microphone's rate.
#[derive(Debug, Clone)]
pub struct Heard {
    samples: Vec<f32>,
    rate: u32,
}

/// Encodes `heard` as an Opus voice note at `path`.
///
/// Blocks while encoding; run it off the UI thread.
///
/// # Errors
///
/// Fails when nothing was heard or the file cannot be encoded or written.
pub fn save(heard: &Heard, path: &Path) -> Result<Recording, RecordError> {
    let samples = resample(&heard.samples, heard.rate, RATE);
    if samples.is_empty() {
        return Err(RecordError::Empty);
    }
    encode(&samples, path)?;
    Ok(Recording {
        seconds: u32::try_from(samples.len() / RATE as usize).unwrap_or(u32::MAX),
        waveform: waveform(&samples),
    })
}

/// `samples` at `to` instead of `from` samples a second, by straight lines
/// between neighbours: plenty for a voice.
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "positions in a recording are far below f64's exact range"
)]
fn resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || samples.is_empty() {
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

/// Loudness over `samples`' length: the peak of each of 64 stretches,
/// scaled so the loudest is 100.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "values are clamped to 0..=100 first"
)]
fn waveform(samples: &[f32]) -> Vec<u8> {
    let stretch = samples.len().div_ceil(BARS).max(1);
    let peaks: Vec<f32> = samples
        .chunks(stretch)
        .map(|chunk| {
            chunk
                .iter()
                .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
        })
        .collect();
    let loudest = peaks
        .iter()
        .copied()
        .fold(0.0_f32, f32::max)
        .max(f32::EPSILON);
    peaks
        .iter()
        .map(|peak| (peak / loudest * 100.0).round().clamp(0.0, 100.0) as u8)
        .collect()
}

fn encode(samples: &[f32], path: &Path) -> Result<(), ffmpeg::Error> {
    ffmpeg::init()?;
    let mut output = format::output_as(path, "ogg")?;
    let codec = ffmpeg::encoder::find(codec::Id::OPUS).ok_or(ffmpeg::Error::EncoderNotFound)?;
    let mut stream = output.add_stream(codec)?;
    let mut encoder = codec::context::Context::new_with_codec(codec)
        .encoder()
        .audio()?;
    encoder.set_rate(RATE_I32);
    encoder.set_channel_layout(ChannelLayout::MONO);
    encoder.set_format(format::Sample::F32(format::sample::Type::Planar));
    encoder.set_bit_rate(32_000);
    encoder.set_time_base((1, RATE_I32));
    // FFmpeg's own Opus encoder is marked experimental.
    let mut options = Dictionary::new();
    options.set("strict", "experimental");
    let mut encoder = encoder.open_as_with(codec, options)?;
    stream.set_parameters(&encoder);
    output.write_header()?;
    let stream_time_base = output
        .stream(0)
        .map_or(encoder.time_base(), |stream| stream.time_base());

    let frame_size = usize::try_from(encoder.frame_size()).unwrap_or(960).max(1);
    let mut pts = 0_i64;
    for chunk in samples.chunks(frame_size) {
        let mut audio = frame::Audio::new(
            format::Sample::F32(format::sample::Type::Planar),
            frame_size,
            ChannelLayout::MONO,
        );
        audio.set_rate(RATE);
        audio.set_pts(Some(pts));
        let plane = audio.plane_mut::<f32>(0);
        plane[..chunk.len()].copy_from_slice(chunk);
        plane[chunk.len()..].fill(0.0);
        pts += i64::try_from(frame_size).unwrap_or(i64::MAX);
        encoder.send_frame(&audio)?;
        write_packets(&mut encoder, &mut output, stream_time_base)?;
    }
    encoder.send_eof()?;
    write_packets(&mut encoder, &mut output, stream_time_base)?;
    output.write_trailer()
}

fn write_packets(
    encoder: &mut ffmpeg::encoder::Audio,
    output: &mut format::context::Output,
    stream_time_base: ffmpeg::Rational,
) -> Result<(), ffmpeg::Error> {
    let mut packet = ffmpeg::Packet::empty();
    while encoder.receive_packet(&mut packet).is_ok() {
        packet.set_stream(0);
        packet.rescale_ts(encoder.time_base(), stream_time_base);
        packet.write_interleaved(output)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_a_voice_note_ffmpeg_reads_back() -> Result<(), Box<dyn std::error::Error>> {
        let rate = 44_100;
        #[expect(clippy::cast_precision_loss, reason = "test indices are small")]
        let tone: Vec<f32> = (0..rate * 2)
            .map(|index| (index as f32 * 0.05).sin() * 0.5)
            .collect();
        let path = std::env::temp_dir().join("natizap-record-test.ogg");
        let recording = save(
            &Heard {
                samples: tone,
                rate: 44_100,
            },
            &path,
        )?;
        assert_eq!(recording.seconds, 2);
        assert_eq!(recording.waveform.len(), BARS);
        let probed = crate::probe(&path)?;
        let seconds = probed
            .duration
            .map_or_default(|duration| duration.as_secs_f32());
        assert!((1.9..2.2).contains(&seconds), "plays {seconds}s");
        Ok(())
    }
}
