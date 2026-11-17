//! The WhatsApp protocol client shared by every feature.
//!
//! [`Protocol`] owns the `whatsapp-rust` client: it opens the local key
//! store, connects, and fans protocol events out to each feature that
//! [subscribes](Protocol::subscribe). Features translate those events into
//! their own types; nothing outside this crate starts or stops the client.

mod actions;
mod chat;
mod content;
mod fanout;
mod markup;
mod media;
mod outgoing;
mod profile;
mod protocol;

pub use actions::{ActionError, Reply, Sent, Unread};
pub use chat::ChatId;
pub use content::{Content, ContentKind, Quote};
pub use markup::{Mark, Marked};
pub use media::{HdThumbnail, Media};
pub use outgoing::{Outgoing, SentMedia};
pub use protocol::{Protocol, StartError};
pub use whatsapp_rust;
