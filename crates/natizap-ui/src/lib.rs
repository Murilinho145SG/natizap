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
mod copied_files;
mod frame;
mod gallery;
mod input;
mod input_element;
mod input_handler;
mod input_keys;
mod input_layout;
pub mod motion;
mod text_boundary;
pub mod theme;

pub use assets::{Assets, Icon, load_fonts};
pub use avatar::{avatar, name_color, name_tint, status_ring};
pub use components::alert::{Alert, AlertTone};
pub use components::alert_dialog::AlertDialog;
pub use components::badge::{BadgeTone, badge, count_label, verified_seal};
pub use components::button::{Button, ButtonSize, ButtonVariant};
pub use components::code_block::code_block;
pub use components::color_picker::{ColorPicked, ColorPicker};
pub use components::dialog::dialog;
pub use components::menu::Menu;
pub use components::progress::progress;
pub use components::rail::{RAIL_AVATAR, RAIL_WIDTH, Rail, rail_avatar};
pub use components::scrollbar::{ScrollSource, scrollbar};
pub use components::separator::separator;
pub use components::settings_window::{SettingsClosed, SettingsPage, SettingsWindow};
pub use components::skeleton::skeleton;
pub use components::splash::{SPLASH_INTRO, SPLASH_OUT, splash};
pub use components::switch::Switch;
pub use components::tooltip::Tooltip;
pub use frame::window_frame;
pub use gallery::Gallery;
pub use input::{FilesPasted, Submitted, TextInput};
pub use input_keys::bind_text_input_keys;
