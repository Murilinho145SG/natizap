//! Stickers: the account's favorites, read from WhatsApp and kept on disk,
//! and the picker to send one from.
//!
//! [`StickerShelf`] keeps the favorites in sync with the phone and their
//! files downloaded; [`StickerPicker`] shows them to pick one, emitting
//! [`StickerPicked`].

mod picker;
mod shelf;
mod store;

pub use picker::{StickerPicked, StickerPicker};
pub use shelf::{Favorite, StickerShelf};
