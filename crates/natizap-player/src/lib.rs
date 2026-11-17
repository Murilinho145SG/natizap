//! The full-window video player.
//!
//! Decoding and sound come from `natizap-playback`; this crate draws the
//! pictures and the controls over the window.

mod view;

pub use view::{PlayerClosed, PlayerView};
