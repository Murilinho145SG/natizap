//! The WhatsApp protocol client shared by every feature.
//!
//! [`Protocol`] owns the `whatsapp-rust` client: it opens the local key
//! store, connects, and fans protocol events out to each feature that
//! [subscribes](Protocol::subscribe). Features translate those events into
//! their own types; nothing outside this crate starts or stops the client.

mod actions;
mod answer;
mod business;
mod buttons;
mod call_log;
mod calls;
mod chat;
mod chat_actions;
mod content;
mod fanout;
mod group;
mod link;
mod markup;
mod media;
mod outgoing;
mod own;
mod polls;
mod profile;
mod protocol;
mod read_elsewhere;
mod status;
mod status_link;
mod stickers;
mod typing;

pub use actions::{ActionError, Anchor, Disappearing, PinFor, Reply, Sent, Unread};
pub use answer::Answer;
pub use business::{Business, DayHours, Hours};
pub use buttons::{Button, ButtonAction, Buttons};
pub use call_log::{CallFollower, CallOutcome, CallRecord};
pub use calls::{MissedCall, Ringing};
pub use chat::{ChatId, PersonName};
pub use chat_actions::ChatAction;
pub use content::{Card, Content, ContentKind, Extra, Forwarding, Poll, Quote};
pub use group::{GroupInfo, Member, Role};
pub use link::LinkPreview;
pub use markup::{LinkKind, Mark, Marked};
pub use media::{HdThumbnail, Media, MediaOrigin, Received};
pub use outgoing::{Forward, Outgoing, SentMedia};
pub use own::Me;
pub use polls::{Account, SealedVote};
pub use profile::Profile;
pub use protocol::{Protocol, StartError};
pub use read_elsewhere::ReadElsewhere;
pub use status_link::StatusLink;
pub use stickers::Sticker;
pub use typing::{Activity, STILL_TYPING, Typing};
pub use whatsapp_rust;
