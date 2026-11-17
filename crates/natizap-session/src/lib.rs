//! Linking natizap to the user's phone: pairing, connection state and the
//! pairing screen.
//!
//! [`Session`] follows the protocol client's lifecycle events and exposes
//! where the link stands; [`PairingView`] renders every state before the
//! session is connected.

mod pairing_view;
mod qr;
mod session;

pub use pairing_view::PairingView;
pub use qr::QrMatrix;
pub use session::{Session, SessionError, SessionState};
