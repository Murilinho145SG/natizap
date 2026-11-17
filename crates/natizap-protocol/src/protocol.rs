use std::path::PathBuf;
use std::sync::Arc;

use gpui::{Context, EventEmitter, Task};
use gpui_tokio::{JoinError, Tokio};
use whatsapp_rust::prelude::*;
use whatsapp_rust::wacore::store::DevicePropsOverride;

use crate::fanout::Fanout;

/// Name shown on the phone under "Linked devices".
const DEVICE_NAME: &str = "natizap";

/// Why the protocol client could not start.
#[derive(Debug, thiserror::Error)]
pub enum StartError {
    /// The local key store could not be opened.
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
}

/// The WhatsApp client, running as a linked device.
///
/// Create it with [`Protocol::new`] inside `cx.new`. Features receive protocol
/// events through [`Protocol::subscribe`] and learn about start failures by
/// subscribing to the entity's [`StartError`] events. Dropping it stops the
/// client.
pub struct Protocol {
    store_path: PathBuf,
    fanout: Fanout,
    client: Task<()>,
}

impl EventEmitter<Arc<StartError>> for Protocol {}

impl std::fmt::Debug for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Protocol")
            .field("store_path", &self.store_path)
            .finish_non_exhaustive()
    }
}

impl Protocol {
    /// Starts a client whose keys live in the SQLite database at
    /// `store_path`.
    ///
    /// The client connects once the current app update finishes, so features
    /// set up alongside it receive every event from the first one.
    /// Requires `gpui_tokio::init` to have been called.
    pub fn new(store_path: PathBuf, cx: &mut Context<Self>) -> Self {
        let fanout = Fanout::default();
        let client = start(store_path.clone(), fanout.clone(), cx);
        Self {
            store_path,
            fanout,
            client,
        }
    }

    /// Delivers every future event of the given kinds, in order, until the
    /// receiver is dropped. Subscriptions survive [`Protocol::restart`].
    #[must_use]
    pub fn subscribe(&self, kinds: &[EventKind]) -> async_channel::Receiver<Arc<Event>> {
        self.fanout.subscribe(EventInterest::of(kinds))
    }

    /// Stops the client and connects again from scratch, e.g. to get fresh
    /// pairing QR codes after the previous ones expired.
    pub fn restart(&mut self, cx: &mut Context<Self>) {
        // Replacing the task drops, and so stops, the running client.
        self.client = start(self.store_path.clone(), self.fanout.clone(), cx);
    }
}

/// Runs the client on Tokio, starting on the next app tick; start failures
/// are emitted on the entity.
fn start(store_path: PathBuf, fanout: Fanout, cx: &mut Context<Protocol>) -> Task<()> {
    cx.spawn(async move |this, cx| {
        let client: Task<Result<Result<(), StartError>, JoinError>> =
            Tokio::spawn(cx, run(store_path, fanout));
        let error = match client.await {
            Ok(Ok(())) => return,
            Ok(Err(error)) => error,
            Err(join_error) => {
                // Only a panic in the client ends up here; cancellation drops
                // this task along with the client.
                tracing::error!(%join_error, "protocol client crashed");
                return;
            }
        };
        tracing::error!(%error, "protocol client failed to start");
        let _ = this.update(cx, |_, cx| cx.emit(Arc::new(error)));
    })
}

async fn run(store_path: PathBuf, fanout: Fanout) -> Result<(), StartError> {
    let store = SqliteStore::new(&store_path.to_string_lossy())
        .await
        .map_err(|source| StartError::Store {
            path: store_path,
            source,
        })?;

    let bot = Bot::builder()
        .with_backend(store)
        .with_device_props(DevicePropsOverride::new().with_os(DEVICE_NAME))
        .with_event_handler(fanout)
        .build()
        .await
        .map_err(StartError::Client)?;

    bot.run().await;
    Ok(())
}
