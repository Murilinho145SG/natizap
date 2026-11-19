//! Sound for a call: the microphone and the speaker, in the rate and the
//! frames a call's codec takes, whatever the sound card's own.
//!
//! Everything happens in the sound card's callbacks, which never wait:
//! frames go to the call and come from it through channels tried, never
//! awaited, so a slow call drops a frame instead of stalling the card.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use cpal::Stream;
use cpal::traits::{DeviceTrait as _, StreamTrait as _};

use crate::audio::{closest_f32, open_microphone};
use crate::devices::{self, CallDevices};
use crate::opus::OpusDecoder;
use crate::resample::Resampler;
use crate::voice_processing::VoiceProcessing;

mod sides;

use sides::{Microphone, Speaker, Volume, to_i16};

/// How much sound the card takes at a time in a call: what it holds is
/// heard as delay.
const CALL_PERIOD: std::time::Duration = std::time::Duration::from_millis(10);
/// Frames waiting for the call: past this, the oldest goes, so a call that
/// starts consuming late does not keep the delay it built up.
const MICROPHONE_BACKLOG: usize = 2;
/// Frames from the call waiting for the speaker's callback.
const SPEAKER_BACKLOG: usize = 8;

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
    /// Received frames loud enough to be a voice, not silence.
    voiced: AtomicU64,
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
    format: CallFormat,
    /// The call's ends of the sound, kept to switch devices without the
    /// call noticing.
    heard: async_channel::Sender<Vec<i16>>,
    said: async_channel::Receiver<Vec<i16>>,
    voice: VoiceProcessing,
    meter: Arc<Meter>,
    volume: Arc<Volume>,
}

impl Drop for CallAudio {
    fn drop(&mut self) {
        let read = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        let meter = &self.meter;
        tracing::info!(
            sent = read(&meter.sent),
            unsent = read(&meter.unsent),
            received = read(&meter.received),
            voiced = read(&meter.voiced),
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
    /// Opens `devices`' microphone and speaker, the system's own where
    /// none is chosen or the chosen one is gone, for a call in `format`.
    ///
    /// # Errors
    ///
    /// Fails when either device is missing or refuses to open.
    pub fn open(
        format: CallFormat,
        devices: &CallDevices,
    ) -> Result<(Self, CallPorts), CallAudioError> {
        let (heard, microphone) = async_channel::bounded(MICROPHONE_BACKLOG);
        let (speaker, said) = async_channel::bounded(SPEAKER_BACKLOG);
        let meter = Arc::new(Meter::default());
        let voice = VoiceProcessing::new(format.rate);
        let volume = Arc::new(Volume::default());
        let (input, output) = streams(format, devices, (&heard, &said), &voice, (&meter, &volume))?;
        let calls = Self {
            _microphone: input,
            _speaker: output,
            format,
            heard,
            said,
            voice,
            meter: Arc::clone(&meter),
            volume,
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

impl CallAudio {
    /// How many frames from the other end held a voice, not silence, so
    /// far: what changes between two looks is whether they spoke.
    #[must_use]
    pub fn voiced_frames(&self) -> u64 {
        self.meter.voiced.load(Ordering::Relaxed)
    }

    /// Plays the other end `gain` times as loud as they sent: 0 is silent,
    /// and up to twice as loud for someone hard to hear.
    pub fn set_volume(&self, gain: f32) {
        self.volume.set(gain);
    }

    /// Moves the call to `devices`' microphone and speaker; the call goes on
    /// as it was.
    ///
    /// # Errors
    ///
    /// Fails when either device is missing or refuses to open; the devices
    /// in use stay then.
    #[expect(
        clippy::used_underscore_binding,
        reason = "the streams are held only to keep the devices open"
    )]
    pub fn switch(&mut self, devices: &CallDevices) -> Result<(), CallAudioError> {
        let (input, output) = streams(
            self.format,
            devices,
            (&self.heard, &self.said),
            &self.voice,
            (&self.meter, &self.volume),
        )?;
        self._microphone = input;
        self._speaker = output;
        Ok(())
    }
}

/// The microphone's and the speaker's streams for `devices`.
fn streams(
    format: CallFormat,
    devices: &CallDevices,
    (heard, said): (
        &async_channel::Sender<Vec<i16>>,
        &async_channel::Receiver<Vec<i16>>,
    ),
    voice: &VoiceProcessing,
    (meter, volume): (&Arc<Meter>, &Arc<Volume>),
) -> Result<(Stream, Stream), CallAudioError> {
    let input =
        devices::microphone(devices.microphone.as_deref()).ok_or(CallAudioError::NoMicrophone)?;
    let output = devices::speaker(devices.speaker.as_deref()).ok_or(CallAudioError::NoSpeaker)?;
    Ok((
        listen(
            &input,
            format,
            heard.clone(),
            voice.clone(),
            Arc::clone(meter),
        )?,
        play(
            &output,
            format,
            said.clone(),
            voice.clone(),
            (Arc::clone(meter), Arc::clone(volume)),
        )?,
    ))
}

fn listen(
    device: &cpal::Device,
    format: CallFormat,
    heard: async_channel::Sender<Vec<i16>>,
    voice: VoiceProcessing,
    meter: Arc<Meter>,
) -> Result<Stream, CallAudioError> {
    let (stream, config) = open_microphone(
        device,
        CALL_PERIOD,
        |config| {
            let mut microphone = Microphone::new(*config, format, heard, voice, meter);
            move |input: &[f32]| microphone.hear(input)
        },
        |error| tracing::warn!(%error, "microphone failed during a call"),
    )
    .map_err(CallAudioError::Device)?;
    tracing::info!(
        rate = config.sample_rate,
        channels = config.channels,
        "call microphone open"
    );
    stream.play().map_err(device_error)?;
    Ok(stream)
}

fn play(
    device: &cpal::Device,
    format: CallFormat,
    said: async_channel::Receiver<Vec<i16>>,
    voice: VoiceProcessing,
    (meter, volume): (Arc<Meter>, Arc<Volume>),
) -> Result<Stream, CallAudioError> {
    let config = closest_f32(
        &device.default_output_config().map_err(device_error)?,
        device.supported_output_configs().map_err(device_error)?,
        CALL_PERIOD,
    )
    .ok_or_else(|| CallAudioError::Device("the speaker takes no f32 samples".into()))?;
    tracing::info!(
        rate = config.sample_rate,
        channels = config.channels,
        "call speaker open"
    );
    let mut speaker = Speaker::new(config, format, said, voice, meter, volume);
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
