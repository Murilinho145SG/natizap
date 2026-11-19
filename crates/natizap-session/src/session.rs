use std::sync::Arc;
use std::time::Duration;

use gpui::{Context, Entity, Task};
use natizap_protocol::whatsapp_rust::prelude::{Event, EventKind};
use natizap_protocol::{Protocol, StartError};

use crate::QrMatrix;

/// Protocol events that move the session through its lifecycle.
const LIFECYCLE_EVENTS: &[EventKind] = &[
    EventKind::PairingQrCode,
    EventKind::PairingQrCodesExhausted,
    EventKind::PairSuccess,
    EventKind::PairError,
    EventKind::QrScannedWithoutMultidevice,
    EventKind::Connected,
    EventKind::Disconnected,
    EventKind::LoggedOut,
    EventKind::StreamReplaced,
    EventKind::ClientOutdated,
    EventKind::ConnectFailure,
    EventKind::TemporaryBan,
];

/// Where the session is in its lifecycle.
#[derive(Debug)]
pub enum SessionState {
    /// Opening the local store and connecting to WhatsApp.
    Starting,
    /// Not linked yet: the phone must scan this QR code. WhatsApp rotates the
    /// code periodically, so the state is replaced on each rotation.
    AwaitingPairing(QrMatrix),
    /// WhatsApp stopped issuing QR codes because none was scanned in time;
    /// [`Session::restart`] asks for new ones.
    PairingExpired,
    /// The phone accepted the link; the first connection is being set up.
    Paired,
    /// Linked and authenticated.
    Connected,
    /// The connection dropped after being established; the client is
    /// reconnecting on its own.
    Reconnecting,
    /// The phone unlinked this device; [`Session::restart`] starts a new
    /// pairing.
    LoggedOut,
    /// The session stopped and needs the user's attention.
    Failed(SessionError),
}

/// Why a session stopped.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// The protocol client could not start.
    #[error(transparent)]
    Start(Arc<StartError>),
    /// WhatsApp issued a pairing payload that does not fit in a QR code.
    #[error("received a pairing code that cannot be shown as a QR code")]
    Qr(#[source] qrcode::types::QrError),
    /// The phone scanned the code but linking failed.
    #[error("linking failed: {0}")]
    Pairing(String),
    /// The phone that scanned the code does not support linked devices.
    #[error("the phone's WhatsApp does not support linked devices; update it and try again")]
    NoMultiDevice,
    /// Another client took over this device's connection, usually a second
    /// natizap window or process.
    #[error("this account was opened in another natizap instance")]
    Replaced,
    /// WhatsApp rejected this client's protocol version.
    #[error("WhatsApp rejected this client version; natizap needs an update")]
    Outdated,
    /// WhatsApp refused the connection.
    #[error("WhatsApp refused the connection: {0}")]
    Refused(String),
    /// The account is temporarily banned from linked devices.
    #[error("the account is temporarily banned for {}s", .expires_in.as_secs())]
    TemporarilyBanned {
        /// Time left until the ban expires.
        expires_in: Duration,
    },
}

/// Messages from the protocol client (on Tokio) to the session (on the UI
/// thread).
enum Update {
    Pairing(QrMatrix),
    PairingExpired,
    Paired,
    Connected,
    Disconnected,
    LoggedOut,
    Failed(SessionError),
}

impl Update {
    fn from_event(event: &Event) -> Option<Self> {
        let update = match event {
            Event::PairingQrCode(qr) => match QrMatrix::encode(&qr.code) {
                Ok(qr) => Self::Pairing(qr),
                Err(error) => Self::Failed(SessionError::Qr(error)),
            },
            Event::PairingQrCodesExhausted(_) => Self::PairingExpired,
            Event::PairSuccess(_) => Self::Paired,
            Event::PairError(e) => Self::Failed(SessionError::Pairing(e.error.clone())),
            Event::QrScannedWithoutMultidevice(_) => Self::Failed(SessionError::NoMultiDevice),
            Event::Connected(_) => Self::Connected,
            Event::Disconnected(_) => Self::Disconnected,
            Event::LoggedOut(_) => Self::LoggedOut,
            Event::StreamReplaced(_) => Self::Failed(SessionError::Replaced),
            Event::ClientOutdated(_) => Self::Failed(SessionError::Outdated),
            Event::ConnectFailure(f) => Self::Failed(SessionError::Refused(
                f.message
                    .clone()
                    .unwrap_or_else(|| format!("{:?}", f.reason)),
            )),
            Event::TemporaryBan(ban) => Self::Failed(SessionError::TemporarilyBanned {
                expires_in: ban.expire,
            }),
            _ => return None,
        };
        Some(update)
    }
}

/// Where the link between natizap and the user's phone stands.
///
/// Create it with [`Session::new`] inside `cx.new`; it notifies observers on
/// every state change.
#[derive(Debug)]
pub struct Session {
    protocol: Entity<Protocol>,
    state: SessionState,
    _events: Task<()>,
}

impl Session {
    /// Follows the lifecycle of `protocol`'s client.
    pub fn new(protocol: Entity<Protocol>, cx: &mut Context<Self>) -> Self {
        let events = protocol.read(cx).subscribe(LIFECYCLE_EVENTS);
        let events = cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                let Some(update) = Update::from_event(&event) else {
                    continue;
                };
                let applied = this.update(cx, |session, cx| {
                    session.apply(update);
                    cx.notify();
                });
                if applied.is_err() {
                    break;
                }
            }
        });
        cx.subscribe(&protocol, |session, _, error: &Arc<StartError>, cx| {
            session.apply(Update::Failed(SessionError::Start(error.clone())));
            cx.notify();
        })
        .detach();
        Self {
            protocol,
            state: SessionState::Starting,
            _events: events,
        }
    }

    /// The current state.
    #[must_use]
    pub fn state(&self) -> &SessionState {
        &self.state
    }

    /// Whether the phone has linked this device, even if the connection is
    /// momentarily down.
    #[must_use]
    pub fn is_linked(&self) -> bool {
        matches!(
            self.state,
            SessionState::Paired | SessionState::Connected | SessionState::Reconnecting
        )
    }

    /// Restarts the protocol client from scratch, e.g. to get fresh QR codes
    /// after [`SessionState::PairingExpired`].
    pub fn restart(&mut self, cx: &mut Context<Self>) {
        self.protocol.update(cx, Protocol::restart);
        self.state = SessionState::Starting;
        cx.notify();
    }

    fn apply(&mut self, update: Update) {
        self.state = match update {
            Update::Pairing(qr) => {
                tracing::info!("pairing QR code issued");
                SessionState::AwaitingPairing(qr)
            }
            Update::PairingExpired => {
                tracing::info!("pairing QR codes expired");
                SessionState::PairingExpired
            }
            Update::Paired => {
                tracing::info!("device linked");
                SessionState::Paired
            }
            Update::Connected => {
                tracing::info!("session connected");
                SessionState::Connected
            }
            Update::Disconnected => {
                tracing::info!("session disconnected");
                match self.state {
                    // Pairing reconnects as part of the flow; only a dropped
                    // live connection is worth showing.
                    SessionState::Connected => SessionState::Reconnecting,
                    _ => return,
                }
            }
            Update::LoggedOut => {
                tracing::warn!("device was unlinked");
                SessionState::LoggedOut
            }
            Update::Failed(error) => {
                tracing::error!(%error, "session failed");
                SessionState::Failed(error)
            }
        };
    }
}
