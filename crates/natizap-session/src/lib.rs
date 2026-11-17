//! Connection to WhatsApp as a linked device: pairing, connection state and
//! the pairing screen.
//!
//! [`Session`] owns the protocol client and runs it on the Tokio runtime
//! registered with `gpui_tokio`. [`PairingView`] renders every state before
//! the session is connected.

mod pairing_view;
mod qr;
mod session;

pub use pairing_view::PairingView;
pub use qr::QrMatrix;
pub use session::{Session, SessionError, SessionState};
