//! Calls: the account's call history, missed and answered, with
//! back-to-back calls gathered, and the Calls place that lists it.

mod group;
mod log;
mod pane;
mod store;

pub use group::CallGroup;
pub use log::CallLog;
pub use pane::{CallChatRequested, CallsPane};
