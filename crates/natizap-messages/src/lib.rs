//! Conversations: every message natizap has seen, the open chat's messages
//! and sending new ones.
//!
//! [`MessageLog`] stores history and live messages and keeps the open chat
//! current; [`ConversationView`] renders it with a composer and plays voice
//! notes inside their bubbles.

mod authors;
mod bubble;
mod emoji;
mod extra_view;
mod items;
mod library;
mod log;
mod media;
mod media_browser;
mod media_view;
mod message;
mod outbox;
mod overview;
mod poll;
mod poll_composer;
mod presence;
mod preview;
mod quote;
mod rich_text;
mod send_preview;
mod store;
mod sync;
mod text_pill;
mod thumbs;
mod view;
mod voice;
mod voice_view;

pub use log::{ForwardRequested, MessageLog, MessageSent, OpenChat, PlayRequested};
pub use overview::{ChatOverview, MediaTile, TileImage};
pub use view::{
    ChatClosed, ChatRequested, ContactsRequested, ConversationView, MuteRequested,
    ProfileRequested, StickerFavoriteToggled,
};
