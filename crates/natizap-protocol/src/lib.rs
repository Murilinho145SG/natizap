//! The WhatsApp protocol client shared by every feature.
//!
//! [`Protocol`] owns the `whatsapp-rust` client: it opens the local key
//! store, connects, and fans protocol events out to each feature that
//! [subscribes](Protocol::subscribe). Features translate those events into
//! their own types; nothing outside this crate starts or stops the client.

mod actions;
mod business;
mod chat;
mod content;
mod fanout;
mod group;
mod markup;
mod media;
mod outgoing;
mod profile;
mod protocol;

pub use actions::{ActionError, Disappearing, PinFor, Reply, Sent, Unread};
pub use business::{Business, DayHours, Hours};
pub use chat::ChatId;
pub use content::{Content, ContentKind, Quote};
pub use group::GroupInfo;
pub use markup::{LinkKind, Mark, Marked};
pub use media::{HdThumbnail, Media};
pub use outgoing::{Forward, Outgoing, SentMedia};
pub use profile::Profile;
pub use protocol::{Protocol, StartError};
pub use whatsapp_rust;
