//! natizap's shared look: color and type tokens, bundled fonts and icons, and
//! the window frame with its title bar.
//!
//! Features style their views from [`theme`] so the app reads as one piece;
//! the binary registers [`Assets`] and calls [`load_fonts`] and
//! [`bind_text_input_keys`] at startup.

mod assets;
mod frame;
mod input;
mod input_element;
pub mod theme;

pub use assets::{Assets, Icon, load_fonts};
pub use frame::window_frame;
pub use input::{Submitted, TextInput, bind_text_input_keys};
