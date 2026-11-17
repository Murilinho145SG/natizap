use std::path::PathBuf;

use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui::{Context, Task};
use gpui_tokio::{JoinError, Tokio};
use whatsapp_rust::prelude::*;
use whatsapp_rust::wacore::store::DevicePropsOverride;

use crate::QrMatrix;

/// Name shown on the phone under "Linked devices".
const DEVICE_NAME: &str = "natizap";

/// Where the session is in its lifecycle.
#[derive(Debug)]
pub enum SessionState {
    /// Opening the local store and connecting to WhatsApp.
    Starting,
    /// Not linked yet: the phone must scan this QR code. WhatsApp rotates the
    /// code periodically, so the state is replaced on each rotation.
    AwaitingPairing(QrMatrix),
    /// Linked and authenticated.
    Connected,
    /// The phone unlinked this device; a new pairing is required.
    LoggedOut,
    /// The session could not start.
    Failed(SessionError),
}

/// Why a session could not start.
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
}

/// Messages from the protocol client (on Tokio) to the session (on the UI
/// thread).
enum Update {
    Pairing(QrMatrix),
    Connected,
    LoggedOut,
    Failed(SessionError),
}

/// The connection to WhatsApp as a linked device.
///
/// Create it with [`Session::new`] inside `cx.new`; it starts connecting
/// immediately and notifies observers on every state change. Dropping it
/// stops the client.
#[derive(Debug)]
pub struct Session {
    state: SessionState,
    _client: Task<Result<(), JoinError>>,
    _updates: Task<()>,
}

impl Session {
    /// Starts a session whose keys and history live in the SQLite database at
    /// `store_path`.
    ///
    /// Requires `gpui_tokio::init` to have been called.
    pub fn new(store_path: PathBuf, cx: &mut Context<Self>) -> Self {
        let (tx, mut rx) = mpsc::unbounded();
        let client = Tokio::spawn(cx, run_client(store_path, tx));
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
        Self {
            state: SessionState::Starting,
            _client: client,
            _updates: updates,
        }
    }

    /// The current state.
    #[must_use]
    pub fn state(&self) -> &SessionState {
        &self.state
    }

    fn apply(&mut self, update: Update) {
        self.state = match update {
            Update::Pairing(qr) => {
                tracing::info!("pairing QR code issued");
                SessionState::AwaitingPairing(qr)
            }
            Update::Connected => {
                tracing::info!("session connected");
                SessionState::Connected
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

async fn run_client(store_path: PathBuf, tx: mpsc::UnboundedSender<Update>) {
    if let Err(error) = connect(store_path, tx.clone()).await {
        // The receiver only goes away when the session is dropped, and then
        // nobody is left to show the error.
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

    let on_qr = tx.clone();
    let on_connected = tx.clone();
    let on_logged_out = tx;
    let bot = Bot::builder()
        .with_backend(store)
        .with_device_props(DevicePropsOverride::new().with_os(DEVICE_NAME))
        .on_qr_code(move |code, _valid_for| {
            let update = match QrMatrix::encode(&code) {
                Ok(qr) => Update::Pairing(qr),
                Err(error) => Update::Failed(SessionError::Qr(error)),
            };
            let _ = on_qr.unbounded_send(update);
            async {}
        })
        .on_connected(move |_client| {
            let _ = on_connected.unbounded_send(Update::Connected);
            async {}
        })
        .on_logged_out(move |_info| {
            let _ = on_logged_out.unbounded_send(Update::LoggedOut);
            async {}
        })
        .build()
        .await
        .map_err(SessionError::Client)?;

    bot.run().await;
    Ok(())
}
