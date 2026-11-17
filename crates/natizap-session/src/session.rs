use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui::{Context, Task};
use gpui_tokio::{JoinError, Tokio};
use whatsapp_rust::prelude::*;
use whatsapp_rust::wacore::store::DevicePropsOverride;

use crate::QrMatrix;

/// Name shown on the phone under "Linked devices".
const DEVICE_NAME: &str = "natizap";

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
    /// The local key and history store could not be opened.
    #[error("could not open the local store at {path}")]
    Store {
        /// Location of the store on disk.
        path: PathBuf,
        /// Underlying storage error.
        #[source]
        source: whatsapp_rust::store::error::StoreError,
    },
    /// The protocol client could not be built.
    #[error("could not start the WhatsApp client")]
    Client(#[source] whatsapp_rust::bot::BotBuilderError),
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
                // A negative remainder means the ban already lapsed.
                expires_in: ban.expire.to_std().unwrap_or_default(),
            }),
            _ => return None,
        };
        Some(update)
    }
}

/// The connection to WhatsApp as a linked device.
///
/// Create it with [`Session::new`] inside `cx.new`; it starts connecting
/// immediately and notifies observers on every state change. Dropping it
/// stops the client.
#[derive(Debug)]
pub struct Session {
    store_path: PathBuf,
    state: SessionState,
    _running: Running,
}

/// The protocol client and the task applying its updates; dropping it stops
/// both.
#[derive(Debug)]
struct Running {
    _protocol: Task<Result<(), JoinError>>,
    _updates: Task<()>,
}

impl Session {
    /// Starts a session whose keys and history live in the SQLite database at
    /// `store_path`.
    ///
    /// Requires `gpui_tokio::init` to have been called.
    pub fn new(store_path: PathBuf, cx: &mut Context<Self>) -> Self {
        let running = start(store_path.clone(), cx);
        Self {
            store_path,
            state: SessionState::Starting,
            _running: running,
        }
    }

    /// The current state.
    #[must_use]
    pub fn state(&self) -> &SessionState {
        &self.state
    }

    /// Stops the current client and connects again from scratch, e.g. to get
    /// fresh QR codes after [`SessionState::PairingExpired`].
    pub fn restart(&mut self, cx: &mut Context<Self>) {
        // Replacing the session drops, and so stops, the running client.
        *self = Self::new(self.store_path.clone(), cx);
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

/// Spawns the protocol client on Tokio and the task that applies its updates
/// to the session.
fn start(store_path: PathBuf, cx: &mut Context<Session>) -> Running {
    let (tx, mut rx) = mpsc::unbounded();
    let protocol = Tokio::spawn(cx, run_client(store_path, tx));
    let updates = cx.spawn(async move |this, cx| {
        while let Some(update) = rx.next().await {
            let applied = this.update(cx, |session, cx| {
                session.apply(update);
                cx.notify();
            });
            if applied.is_err() {
                break;
            }
        }
    });
    Running {
        _protocol: protocol,
        _updates: updates,
    }
}

async fn run_client(store_path: PathBuf, tx: mpsc::UnboundedSender<Update>) {
    if let Err(error) = connect(store_path, tx.clone()).await {
        // The receiver only goes away when the session is dropped or
        // restarted, and then nobody is left to show the error.
        let _ = tx.unbounded_send(Update::Failed(error));
    }
}

async fn connect(
    store_path: PathBuf,
    tx: mpsc::UnboundedSender<Update>,
) -> Result<(), SessionError> {
    let store = SqliteStore::new(&store_path.to_string_lossy())
        .await
        .map_err(|source| SessionError::Store {
            path: store_path,
            source,
        })?;

    let bot = Bot::builder()
        .with_backend(store)
        .with_device_props(DevicePropsOverride::new().with_os(DEVICE_NAME))
        .on_event_for(LIFECYCLE_EVENTS, move |event: Arc<Event>, _client| {
            if let Some(update) = Update::from_event(&event) {
                let _ = tx.unbounded_send(update);
            }
            async {}
        })
        .build()
        .await
        .map_err(SessionError::Client)?;

    bot.run().await;
    Ok(())
}
