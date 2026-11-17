//! natizap's shared look: color and type tokens, bundled fonts and icons, and
//! the window frame with its title bar.
//!
//! It is also the component library: buttons, badges, tooltips and the rest,
//! each shown in every state by the [`Gallery`].
//!
//! Features style their views from [`theme`] so the app reads as one piece;
//! the binary registers [`Assets`] and calls [`load_fonts`] and
//! [`bind_text_input_keys`] at startup.

mod assets;
mod avatar;
mod components;
mod frame;
mod gallery;
mod input;
mod input_element;
pub mod theme;

pub use assets::{Assets, Icon, load_fonts};
pub use avatar::{avatar, name_color};
pub use components::alert::{Alert, AlertTone};
pub use components::alert_dialog::AlertDialog;
pub use components::badge::{BadgeTone, badge, count_label};
pub use components::button::{Button, ButtonSize, ButtonVariant};
pub use components::menu::Menu;
pub use components::progress::progress;
pub use components::separator::separator;
pub use components::skeleton::skeleton;
pub use components::tooltip::Tooltip;
pub use frame::window_frame;
pub use gallery::Gallery;
pub use input::{Submitted, TextInput, bind_text_input_keys};
