//! The media engine every player in natizap shares.
//!
//! FFmpeg, built into the binary, demuxes and decodes on a dedicated thread;
//! sound goes to the sound card through `cpal`, and the sound card's progress
//! is the clock video pictures follow. Callers only see [`Playback`]: they
//! steer it and read where it is, and draw it however their feature needs.
//! [`save_poster`] and [`still_jpeg`] take a still from a video,
//! [`probe`] reads how long a file plays and how big its pictures are,
//! [`Recorder`] records voice notes, [`Clip`] plays short sounds held in
//! memory and [`CallAudio`] carries a call's sound to and from the sound
//! card.

mod audio;
mod call_audio;
mod clip;
mod clock;
mod decode;
mod poster;
mod probe;
mod record;
mod resample;
mod speed;
mod turn;

use std::path::Path;
use std::sync::{Arc, mpsc};
use std::time::Duration;

pub use call_audio::{CallAudio, CallAudioError, CallFormat, CallPorts};
pub use clip::{Clip, ClipError};
pub use poster::{MediaFileError, save_poster, still_jpeg};
pub use probe::{Probe, probe};
pub use record::{Heard, RecordError, Recorder, Recording, save as save_recording};
pub use speed::Speed;

use decode::{Command, Shared};

/// One file playing. Playback stops when this is dropped.
pub struct Playback {
    shared: Arc<Shared>,
    commands: mpsc::Sender<Command>,
}

impl std::fmt::Debug for Playback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Playback").finish_non_exhaustive()
    }
}

/// What is known about the file being played.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    /// How long the file is, once the container says so.
    pub duration: Option<Duration>,
    /// Whether the file has pictures, not just sound.
    pub has_video: bool,
    /// Whether everything has been played.
    pub ended: bool,
    /// Why the file cannot be played, if it cannot.
    pub error: Option<String>,
}

/// A decoded picture, BGRA with rows packed back to back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes.
    pub bgra: Vec<u8>,
}

impl Playback {
    /// Starts playing `path` from the beginning.
    #[must_use]
    pub fn start(path: &Path) -> Self {
        let shared = Arc::new(Shared::default());
        let (commands, received) = mpsc::channel();
        let (decoding, path) = (Arc::clone(&shared), path.to_owned());
        // A plain thread: decoding blocks on FFmpeg and must never share a
        // pool with UI or protocol work. Dropping `commands` ends it.
        if let Err(error) = std::thread::Builder::new()
            .name("natizap-playback".into())
            .spawn(move || decode::run(&path, &decoding, &received))
        {
            shared.status().error = Some(error.to_string());
        }
        Self { shared, commands }
    }

    /// Pauses or resumes.
    pub fn set_paused(&self, paused: bool) {
        self.send(Command::Pause(paused));
    }

    /// Jumps to `to`, clamped to the file's length.
    pub fn seek(&self, to: Duration) {
        let to = self.status().duration.map_or(to, |total| to.min(total));
        self.send(Command::Seek(to));
    }

    /// Plays at `speed`, keeping voices at their pitch.
    pub fn set_speed(&self, speed: Speed) {
        self.send(Command::Speed(speed));
    }

    /// Where playback is now.
    #[must_use]
    pub fn position(&self) -> Duration {
        self.shared.clock.position()
    }

    /// What is known about the file.
    #[must_use]
    pub fn status(&self) -> Status {
        self.shared.status().clone()
    }

    /// The newest picture whose time has come, dropping older ones that
    /// were never shown. `None` when the shown picture is still current.
    #[must_use]
    pub fn due_picture(&self) -> Option<Picture> {
        let now = self.position();
        let mut pictures = self.shared.pictures();
        let mut due = None;
        while pictures.front().is_some_and(|picture| picture.at <= now) {
            due = pictures.pop_front();
        }
        due.map(|picture| picture.image)
    }

    fn send(&self, command: Command) {
        // Only fails once the decoder stopped, when there is nothing to steer.
        let _ = self.commands.send(command);
    }
}

/// Why a file cannot be played.
#[derive(Debug, thiserror::Error)]
pub(crate) enum PlaybackError {
    /// FFmpeg could not open or decode the file.
    #[error("{0}")]
    Media(#[from] ffmpeg_next::Error),
    /// There is no sound card to play through.
    #[error("there is no sound output device")]
    NoAudioDevice,
    /// The sound card refused the stream.
    #[error("sound output failed: {0}")]
    Audio(String),
}
