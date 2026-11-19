//! Calls: the account's call history, missed and answered, with
//! back-to-back calls gathered, and the Calls place that lists it; and
//! the calls natizap itself takes part in, ringing, under way in a bar
//! across the main window or in a window of their own.

mod bar;
mod desk;
mod group;
mod log;
mod pane;
mod ringtone;
mod settings;
mod store;
mod window;

pub use bar::{CallBar, CallPoppedOut, RingCard};
pub use desk::{CallDesk, CallRang, CallShown, Phase};
pub use group::CallGroup;
pub use log::CallLog;
pub use pane::{CallChatRequested, CallsPane};
pub use ringtone::Ringtone;
pub use settings::CallSettings;
pub use window::CallWindow;
