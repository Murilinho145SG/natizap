//! Conversations: every message natizap has seen, the open chat's messages
//! and sending new ones.
//!
//! [`MessageLog`] stores history and live messages and keeps the open chat
//! current; [`ConversationView`] renders it with a composer and plays voice
//! notes inside their bubbles.

mod bubble;
mod emoji;
mod items;
mod log;
mod media;
mod media_view;
mod message;
mod outbox;
mod preview;
mod store;
mod sync;
mod view;
mod voice;
mod voice_view;

pub use log::{MessageLog, MessageSent, OpenChat, PlayRequested};
pub use view::ConversationView;
