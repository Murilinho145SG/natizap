//! Demuxes and decodes a media file on its own thread.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use ffmpeg::format::Pixel;
use ffmpeg::media::Type;
use ffmpeg::software::scaling;
use ffmpeg::util::frame;
use ffmpeg::{ChannelLayout, Rational, codec, filter};
use ffmpeg_next as ffmpeg;

use crate::audio::AudioOutput;
use crate::clock::Clock;
use crate::{Picture as Image, PlaybackError, Speed, Status};

/// Widest picture worth decoding to; larger videos are scaled down while
/// decoding, since the player never shows them bigger than the window.
const MAX_WIDTH: u32 = 1920;
/// Decoded pictures kept ahead of the clock.
const FRAMES_AHEAD: usize = 12;
/// Decoded sound kept ahead of the sound card.
const AUDIO_AHEAD_SECONDS: f32 = 1.0;
/// How long to wait when the buffers are full or playback is paused.
const IDLE: Duration = Duration::from_millis(5);

/// What the UI asks the decoder to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Command {
    Pause(bool),
    Seek(Duration),
    Speed(Speed),
}

/// A decoded picture to show at `at`.
pub(crate) struct Picture {
    pub(crate) at: Duration,
    pub(crate) image: Image,
}

/// What the decoder shares with the UI.
#[derive(Default)]
pub(crate) struct Shared {
    pub(crate) clock: Arc<Clock>,
    pub(crate) pictures: Mutex<VecDeque<Picture>>,
    pub(crate) status: Mutex<Status>,
}

impl Shared {
    pub(crate) fn status(&self) -> std::sync::MutexGuard<'_, Status> {
        self.status.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn pictures(&self) -> std::sync::MutexGuard<'_, VecDeque<Picture>> {
        self.pictures.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Plays `path` until the command channel closes, reporting failures in the
/// shared status.
pub(crate) fn run(path: &Path, shared: &Shared, commands: &Receiver<Command>) {
    if let Err(error) = play(path, shared, commands) {
        tracing::warn!(%error, path = %path.display(), "playback failed");
        shared.status().error = Some(error.to_string());
    }
}

fn play(path: &Path, shared: &Shared, commands: &Receiver<Command>) -> Result<(), PlaybackError> {
    ffmpeg::init()?;
    let mut input = ffmpeg::format::input(path)?;
    let mut video = input
        .streams()
        .best(Type::Video)
        .map(|stream| VideoTrack::open(&stream))
        .transpose()?;
    let mut audio = match input.streams().best(Type::Audio) {
        Some(stream) => Some(AudioTrack::open(&stream, Arc::clone(&shared.clock))?),
        None => None,
    };
    {
        let mut status = shared.status();
        status.has_video = video.is_some();
        status.duration = u64::try_from(input.duration())
            .ok()
            .map(Duration::from_micros);
    }
    if audio.is_none() {
        shared.clock.restart_at(Duration::ZERO);
    }

    let mut input_done = false;
    let mut skip_before = Duration::ZERO;
    loop {
        let seek_to = match commands.try_recv() {
            Ok(Command::Pause(paused)) => {
                shared.clock.set_paused(paused);
                if let Some(audio) = &audio {
                    audio.output.set_paused(paused);
                }
                None
            }
            Ok(Command::Seek(to)) => Some(to),
            Ok(Command::Speed(speed)) => {
                // Sound already queued was stretched for the old speed, so
                // restart from where playback is now.
                let now = shared.clock.position();
                shared.clock.set_speed(speed.factor());
                if let Some(audio) = &mut audio {
                    audio.set_speed(speed)?;
                }
                Some(now)
            }
            Err(TryRecvError::Disconnected) => return Ok(()),
            Err(TryRecvError::Empty) => None,
        };
        if let Some(to) = seek_to {
            seek(&mut input, video.as_mut(), audio.as_mut(), shared, to)?;
            skip_before = to;
            input_done = false;
            continue;
        }

        let pictures_full = shared.pictures().len() >= FRAMES_AHEAD;
        let audio_full = audio
            .as_ref()
            .is_some_and(|a| a.output.queued_seconds() >= AUDIO_AHEAD_SECONDS);
        if input_done || pictures_full || audio_full {
            if input_done
                && shared.pictures().is_empty()
                && audio
                    .as_ref()
                    .is_none_or(|a| a.output.queued_seconds() == 0.0)
            {
                shared.status().ended = true;
            }
            std::thread::sleep(IDLE);
            continue;
        }

        let mut packet = ffmpeg::Packet::empty();
        match packet.read(&mut input) {
            Ok(()) => {}
            Err(ffmpeg::Error::Eof) => {
                if let Some(video) = &mut video {
                    video.decoder.send_eof()?;
                    video.drain(shared, skip_before)?;
                }
                if let Some(audio) = &mut audio {
                    audio.finish(skip_before)?;
                }
                input_done = true;
                continue;
            }
            Err(error) => return Err(error.into()),
        }
        if let Some(video) = video.as_mut().filter(|v| v.index == packet.stream()) {
            video.decoder.send_packet(&packet)?;
            video.drain(shared, skip_before)?;
        } else if let Some(audio) = audio.as_mut().filter(|a| a.index == packet.stream()) {
            audio.decoder.send_packet(&packet)?;
            audio.drain(skip_before)?;
        }
    }
}

/// Restarts decoding at `to`, dropping everything decoded for before it.
fn seek(
    input: &mut ffmpeg::format::context::Input,
    video: Option<&mut VideoTrack>,
    audio: Option<&mut AudioTrack>,
    shared: &Shared,
    to: Duration,
) -> Result<(), PlaybackError> {
    let micros = i64::try_from(to.as_micros()).unwrap_or(i64::MAX);
    input.seek(micros, ..micros)?;
    if let Some(video) = video {
        video.decoder.flush();
    }
    if let Some(audio) = audio {
        audio.decoder.flush();
        audio.reset_tempo()?;
        audio.output.clear();
    }
    shared.pictures().clear();
    shared.clock.restart_at(to);
    shared.status().ended = false;
    Ok(())
}

struct VideoTrack {
    index: usize,
    time_base: Rational,
    decoder: ffmpeg::decoder::Video,
    scaler: scaling::Context,
    width: u32,
    height: u32,
}

impl VideoTrack {
    fn open(stream: &ffmpeg::format::stream::Stream<'_>) -> Result<Self, PlaybackError> {
        let mut context = codec::context::Context::from_parameters(stream.parameters())?;
        // Count 0 lets FFmpeg pick one thread per core.
        context.set_threading(codec::threading::Config {
            kind: codec::threading::Type::Frame,
            count: 0,
        });
        let decoder = context.decoder().video()?;
        let (width, height) = fit(decoder.width(), decoder.height());
        let scaler = scaling::Context::get(
            decoder.format(),
            decoder.width(),
            decoder.height(),
            Pixel::BGRA,
            width,
            height,
            scaling::Flags::BILINEAR,
        )?;
        Ok(Self {
            index: stream.index(),
            time_base: stream.time_base(),
            decoder,
            scaler,
            width,
            height,
        })
    }

    fn drain(&mut self, shared: &Shared, skip_before: Duration) -> Result<(), PlaybackError> {
        let mut decoded = frame::Video::empty();
        while self.decoder.receive_frame(&mut decoded).is_ok() {
            let at = seconds(decoded.timestamp(), self.time_base);
            if at < skip_before {
                continue;
            }
            let mut bgra = frame::Video::empty();
            self.scaler.run(&decoded, &mut bgra)?;
            shared.pictures().push_back(Picture {
                at,
                image: Image {
                    width: self.width,
                    height: self.height,
                    bgra: packed_rows(&bgra, self.width as usize * 4, self.height as usize),
                },
            });
        }
        Ok(())
    }
}

struct AudioTrack {
    index: usize,
    time_base: Rational,
    decoder: ffmpeg::decoder::Audio,
    layout: ChannelLayout,
    speed: Speed,
    /// Changes speed without changing pitch (`atempo`) and converts to the
    /// sound card's format.
    tempo: filter::Graph,
    output: AudioOutput,
}

impl AudioTrack {
    fn open(
        stream: &ffmpeg::format::stream::Stream<'_>,
        clock: Arc<Clock>,
    ) -> Result<Self, PlaybackError> {
        let decoder = codec::context::Context::from_parameters(stream.parameters())?
            .decoder()
            .audio()?;
        let output = AudioOutput::open(clock)?;
        let layout = if decoder.channel_layout().is_empty() {
            ChannelLayout::default(i32::from(decoder.channels()))
        } else {
            decoder.channel_layout()
        };
        let time_base = stream.time_base();
        let tempo = tempo_graph(&decoder, time_base, &layout, &output, Speed::Normal)?;
        Ok(Self {
            index: stream.index(),
            time_base,
            decoder,
            layout,
            speed: Speed::Normal,
            tempo,
            output,
        })
    }

    fn set_speed(&mut self, speed: Speed) -> Result<(), PlaybackError> {
        self.speed = speed;
        self.reset_tempo()
    }

    /// Rebuilds the tempo graph, dropping sound it still holds.
    fn reset_tempo(&mut self) -> Result<(), PlaybackError> {
        self.tempo = tempo_graph(
            &self.decoder,
            self.time_base,
            &self.layout,
            &self.output,
            self.speed,
        )?;
        Ok(())
    }

    fn drain(&mut self, skip_before: Duration) -> Result<(), PlaybackError> {
        let mut decoded = frame::Audio::empty();
        while self.decoder.receive_frame(&mut decoded).is_ok() {
            if seconds(decoded.timestamp(), self.time_base) < skip_before {
                continue;
            }
            self.graph_node("in")?.source().add(&decoded)?;
            self.play_filtered()?;
        }
        Ok(())
    }

    /// Feeds what the decoder still holds at the end of the file.
    fn finish(&mut self, skip_before: Duration) -> Result<(), PlaybackError> {
        self.decoder.send_eof()?;
        self.drain(skip_before)?;
        self.graph_node("in")?.source().flush()?;
        self.play_filtered()
    }

    fn play_filtered(&mut self) -> Result<(), PlaybackError> {
        let channels = usize::from(self.output.channels);
        let mut filtered = frame::Audio::empty();
        while self.graph_node("out")?.sink().frame(&mut filtered).is_ok() {
            let count = filtered.samples() * channels;
            let samples: Vec<f32> = filtered.data(0)[..count * 4]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|&b| f32::from_ne_bytes(b))
                .collect();
            self.output.push(&samples);
        }
        Ok(())
    }

    fn graph_node(&mut self, name: &str) -> Result<filter::Context, PlaybackError> {
        self.tempo
            .get(name)
            .ok_or(PlaybackError::Media(ffmpeg::Error::FilterNotFound))
    }
}

/// `abuffer` → `atempo` → the output's sample format, rate and layout.
fn tempo_graph(
    decoder: &ffmpeg::decoder::Audio,
    time_base: Rational,
    layout: &ChannelLayout,
    output: &AudioOutput,
    speed: Speed,
) -> Result<filter::Graph, PlaybackError> {
    let find = |name| filter::find(name).ok_or(ffmpeg::Error::FilterNotFound);
    let mut graph = filter::Graph::new();
    let source = format!(
        "time_base={time_base}:sample_rate={}:sample_fmt={}:channel_layout=0x{:x}",
        decoder.rate(),
        decoder.format().name(),
        layout.bits()
    );
    graph.add(&find("abuffer")?, "in", &source)?;
    graph.add(&find("abuffersink")?, "out", "")?;
    // The format goes in the chain: this FFmpeg's `abuffersink` ignores the
    // options `ffmpeg-next` sets on it and hands out planar sound.
    let chain = format!(
        "atempo={},aresample={rate},aformat=sample_fmts=flt:sample_rates={rate}:channel_layouts={}c",
        speed.factor(),
        output.channels,
        rate = output.rate,
    );
    graph.output("in", 0)?.input("out", 0)?.parse(&chain)?;
    graph.validate()?;
    Ok(graph)
}

/// Scales `width`×`height` down to at most [`MAX_WIDTH`] wide, keeping the
/// aspect ratio and even sides (the scaler's chroma planes need them).
fn fit(width: u32, height: u32) -> (u32, u32) {
    if width <= MAX_WIDTH {
        return (width, height);
    }
    let scaled = u64::from(height) * u64::from(MAX_WIDTH) / u64::from(width);
    (MAX_WIDTH, u32::try_from(scaled).unwrap_or(height) & !1)
}

/// `timestamp` in `time_base` units as a duration; unknown stamps count as 0.
fn seconds(timestamp: Option<i64>, time_base: Rational) -> Duration {
    let ticks = timestamp.unwrap_or(0).max(0);
    let (num, den) = (
        i64::from(time_base.numerator()),
        i64::from(time_base.denominator()),
    );
    if den == 0 {
        return Duration::ZERO;
    }
    let micros = i128::from(ticks) * i128::from(num) * 1_000_000 / i128::from(den);
    Duration::from_micros(u64::try_from(micros).unwrap_or(0))
}

/// The picture's pixels with each row's padding removed.
fn packed_rows(frame: &frame::Video, row_bytes: usize, rows: usize) -> Vec<u8> {
    let stride = frame.stride(0);
    let data = frame.data(0);
    let mut pixels = Vec::with_capacity(row_bytes * rows);
    for row in 0..rows {
        pixels.extend_from_slice(&data[row * stride..row * stride + row_bytes]);
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_wide_videos_down_keeping_their_shape() {
        assert_eq!(fit(848, 478), (848, 478));
        assert_eq!(fit(3840, 2160), (1920, 1080));
        assert_eq!(fit(2560, 1081), (1920, 810));
    }

    #[test]
    fn converts_timestamps_from_the_stream_time_base() {
        assert_eq!(
            seconds(Some(90_000), Rational::new(1, 90_000)),
            Duration::from_secs(1)
        );
        assert_eq!(
            seconds(Some(48), Rational::new(1, 24)),
            Duration::from_secs(2)
        );
        assert_eq!(seconds(None, Rational::new(1, 1000)), Duration::ZERO);
    }
}
