//! The WhatsApp protocol client shared by every feature.
//!
//! [`Protocol`] owns the `whatsapp-rust` client: it opens the local key
//! store, connects, and fans protocol events out to each feature that
//! [subscribes](Protocol::subscribe). Features translate those events into
//! their own types; nothing outside this crate starts or stops the client.

mod chat;
mod content;
mod fanout;
mod protocol;

pub use chat::ChatId;
pub use content::{Content, ContentKind};
pub use protocol::{Protocol, StartError};
pub use whatsapp_rust;
