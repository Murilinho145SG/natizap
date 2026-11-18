//! People and businesses: the profile panel that opens from a
//! conversation's header.
//!
//! [`ProfilePanel`] shows a chat's full-size picture, name, number and about
//! line, and a business's public details, fetched when it opens.

mod hours;
mod members;
mod panel;
mod sections;

pub use members::PersonOpened;
pub use panel::{ChatWanted, ProfileClosed, ProfilePanel};
pub use sections::{
    ChatSettings, ChatSummary, DisappearingChosen, MediaRequested, MuteToggled, Tile, TileOpened,
};
