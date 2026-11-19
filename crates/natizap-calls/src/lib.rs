//! Calls: the account's call history, missed and answered, with
//! back-to-back calls gathered, and the Calls place that lists it; and
//! the calls natizap itself takes part in, ringing, under way in a bar
//! across the main window or in a window of their own.

mod bar;
mod choices;
mod desk;
mod group;
mod keys;
mod log;
mod look;
mod pane;
mod panel;
mod pointer;
mod quality;
mod ringtone;
mod screen;
mod settings;
mod stage;
mod store;
mod window;

pub use bar::{CallBar, CallPoppedOut, RingCard};
pub use desk::{
    CallDesk, CallLayout, CallRang, CallShown, CameraState, DeviceKind, Phase, VideoShown,
};
pub use group::CallGroup;
pub use keys::{HangUp, ToggleCamera, ToggleMute, call_keys};
pub use log::CallLog;
pub use pane::{CallBackRequested, CallChatRequested, CallsPane};
pub use panel::VideoPanel;
pub use quality::Quality;
pub use ringtone::Ringtone;
pub use settings::CallSettings;
pub use stage::CallStage;
pub use window::CallWindow;
