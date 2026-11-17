//! natizap's shared look: color and type tokens, bundled fonts and icons, and
//! the window frame with its title bar.
//!
//! Features style their views from [`theme`] so the app reads as one piece;
//! the binary registers [`Assets`] and calls [`load_fonts`] at startup.

mod assets;
mod frame;
pub mod theme;

pub use assets::{Assets, Icon, load_fonts};
pub use frame::window_frame;
