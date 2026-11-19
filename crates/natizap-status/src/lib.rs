//! Status updates: everyone's posts of the last day, seen and unseen, the
//! viewer that plays them, replying to them, and posting your own.

mod book;
mod compose;
mod pane;
mod post;
mod read;
mod store;
mod viewer;

pub use book::{PostError, StatusBook};
pub use compose::{ComposerClosed, StatusComposer};
pub use pane::{PostRequested, StatusOpened, StatusPane};
pub use post::{Post, Poster};
pub use store::Viewer;
pub use viewer::{ReplyBody, StatusClosed, StatusPlayRequested, StatusReply, StatusViewer};
