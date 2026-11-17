//! Conversations: every message natizap has seen, the open chat's messages
//! and sending new ones.
//!
//! [`MessageLog`] stores history and live messages and keeps the open chat
//! current; [`ConversationView`] renders it with a composer.

mod bubble;
mod items;
mod log;
mod message;
mod store;
mod sync;
mod view;

pub use log::{MessageLog, MessageSent, OpenChat};
pub use view::ConversationView;
