//! Conversations: every message natizap has seen, the open chat's messages
//! and sending new ones.
//!
//! [`MessageLog`] stores history and live messages and keeps the open chat
//! current; [`ConversationView`] renders it with a composer and plays voice
//! notes inside their bubbles.

mod authors;
mod bubble;
mod carousel;
mod emoji;
mod extra_view;
mod items;
mod library;
mod log;
mod media;
mod media_browser;
mod media_view;
mod message;
mod overview;
mod poll;
mod poll_composer;
mod poll_view;
mod presence;
mod preview;
mod quote;
mod rich_text;
mod search_view;
mod send_preview;
mod settings_view;
mod store;
mod sync;
mod text_pill;
mod text_search;
mod thumbs;
mod view;
mod voice;
mod voice_view;
mod zoom;

pub use log::{ForwardRequested, MessageLog, MessageSent, OpenChat, StoredStatus, TypingLook};
pub use overview::{ChatOverview, MediaTile, TileImage};
pub use search_view::{MessageSearch, SearchHitOpened};
pub use settings_view::ChatSettings;
pub use view::{
    ChatClosed, ChatRequested, ContactsRequested, ConversationView, MuteRequested, PlayRequested,
    ProfileRequested, StickerFavoriteToggled,
};
