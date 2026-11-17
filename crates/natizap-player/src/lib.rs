//! Plays videos, voice notes and audio inside natizap.
//!
//! FFmpeg, built into the binary, demuxes and decodes on a dedicated thread;
//! sound goes to the sound card through `cpal`, and the sound card's progress
//! is the clock video frames follow. [`PlayerView`] shows it all over the
//! window.

mod audio;
mod clock;
mod decode;
mod view;

pub use view::{PlayerClosed, PlayerView};

/// Why a file cannot be played.
#[derive(Debug, thiserror::Error)]
pub(crate) enum PlayerError {
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
