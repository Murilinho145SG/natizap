//! People and businesses: the profile panel that opens from a
//! conversation's header, and your own profile.
//!
//! [`ProfilePanel`] shows a chat's full-size picture, name, number and about
//! line, and a business's public details, fetched when it opens.
//! [`OwnProfile`] shows yours and changes your name, about and picture.

mod business_view;
mod encryption;
mod hours;
mod members;
mod own;
mod panel;
mod sections;

pub use members::PersonOpened;
pub use own::{OwnProfile, OwnProfileClosed};
pub use panel::{ChatWanted, ProfileClosed, ProfilePanel};
pub use sections::{
    ChatSettings, ChatSummary, DisappearingChosen, MediaRequested, MuteToggled, StarredRequested,
    Tile, TileOpened,
};
