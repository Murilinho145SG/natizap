//! The chat list: every conversation, ordered like on the phone, with unread
//! counts, pins, mutes and the archive.
//!
//! [`ChatList`] keeps the chats in sync with WhatsApp and cached on disk;
//! [`ChatListView`] renders them.

mod book;
mod filters;
mod list;
mod picker;
mod store;
mod sync;
mod view;

pub use list::{ChatList, ChatRow};
pub use picker::{ChatPicker, ChatsPicked, PickerDismissed};
pub use view::{ChatListView, ChatSelected, SettingsRequested};
