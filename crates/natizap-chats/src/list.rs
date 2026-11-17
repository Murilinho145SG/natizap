use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::{AppContext as _, Context, Entity, SharedString, Task};
use natizap_protocol::Protocol;
use natizap_protocol::whatsapp_rust::prelude::Event;

use crate::book::{Chat, ChatBook, ChatId};
use crate::store::ChatStore;
use crate::sync::{CHAT_EVENTS, changes};

/// One line of the chat list, ready to render.
#[derive(Debug, Clone)]
pub struct ChatRow {
    /// The chat's address.
    pub id: ChatId,
    /// Name to show.
    pub title: SharedString,
    /// The latest message, prefixed with its author in groups.
    pub preview: Option<SharedString>,
    /// When the latest message was sent, in Unix seconds.
    pub sent_at: Option<i64>,
    /// Unread messages.
    pub unread: u32,
    /// Marked as unread by hand.
    pub marked_unread: bool,
    /// Pinned to the top.
    pub pinned: bool,
    /// Notifications silenced.
    pub muted: bool,
}

impl ChatRow {
    fn new(chat: &Chat, now: i64) -> Self {
        let preview = chat.preview.as_ref().map(|preview| {
            let text = match (&preview.author, preview.from_me) {
                (_, true) => format!("You: {}", preview.text),
                (Some(author), false) => format!("{author}: {}", preview.text),
                (None, false) => preview.text.clone(),
            };
            SharedString::from(first_line(&text).to_owned())
        });
        Self {
            id: chat.id.clone(),
            title: chat.title().into(),
            preview,
            sent_at: chat.preview.as_ref().map(|preview| preview.sent_at),
            unread: chat.unread,
            marked_unread: chat.marked_unread,
            pinned: chat.pinned_at.is_some(),
            muted: chat.is_muted(now),
        }
    }
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or_default()
}

#[derive(Debug, Clone, Default)]
struct Snapshot {
    rows: Arc<[ChatRow]>,
    archived: usize,
}

/// The user's chats, kept in sync with WhatsApp and cached on disk.
///
/// Opens the cache immediately, so the list shows before the connection is
/// up, then follows history syncs and live updates. Notifies observers when
/// the list changes.
#[derive(Debug)]
pub struct ChatList {
    snapshot: Snapshot,
    _sync: Task<()>,
    _apply: Task<()>,
}

impl ChatList {
    /// Follows `protocol`'s chat events, caching the list at `cache_path`.
    pub fn new(protocol: &Entity<Protocol>, cache_path: PathBuf, cx: &mut Context<Self>) -> Self {
        let events = protocol.read(cx).subscribe(CHAT_EVENTS);
        let (snapshots_tx, snapshots) = async_channel::unbounded();
        let sync = cx.background_spawn(sync(cache_path, events, snapshots_tx));
        let apply = cx.spawn(async move |this, cx| {
            while let Ok(mut snapshot) = snapshots.recv().await {
                // Only the newest list matters if several queued up.
                while let Ok(newer) = snapshots.try_recv() {
                    snapshot = newer;
                }
                let applied = this.update(cx, |list, cx| {
                    list.snapshot = snapshot;
                    cx.notify();
                });
                if applied.is_err() {
                    break;
                }
            }
        });
        Self {
            snapshot: Snapshot::default(),
            _sync: sync,
            _apply: apply,
        }
    }

    /// Chats in display order; archived chats are left out.
    #[must_use]
    pub fn rows(&self) -> &Arc<[ChatRow]> {
        &self.snapshot.rows
    }

    /// How many chats are archived.
    #[must_use]
    pub fn archived_count(&self) -> usize {
        self.snapshot.archived
    }
}

/// Folds protocol events into the book and the cache, off the UI thread, and
/// sends a fresh snapshot after each burst of events.
async fn sync(
    cache_path: PathBuf,
    events: async_channel::Receiver<Arc<Event>>,
    snapshots: async_channel::Sender<Snapshot>,
) {
    let Some(mut store) = open_store(&cache_path) else {
        return;
    };
    let mut book = store.load().unwrap_or_else(|error| {
        tracing::error!(%error, "could not read the chat cache; starting empty");
        ChatBook::default()
    });
    if snapshots.send(snapshot(&book)).await.is_err() {
        return;
    }

    while let Ok(event) = events.recv().await {
        handle(&mut book, &mut store, &event);
        while let Ok(event) = events.try_recv() {
            handle(&mut book, &mut store, &event);
        }
        let dirty = book.take_dirty();
        if let Err(error) = store.save(&book, &dirty) {
            tracing::error!(%error, "could not update the chat cache");
        }
        if snapshots.send(snapshot(&book)).await.is_err() {
            return;
        }
    }
}

fn handle(book: &mut ChatBook, store: &mut ChatStore, event: &Event) {
    if matches!(event, Event::LoggedOut(_)) {
        // Whoever links next may be a different account.
        *book = ChatBook::default();
        if let Err(error) = store.clear() {
            tracing::error!(%error, "could not clear the chat cache");
        }
        return;
    }
    for change in changes(event) {
        book.apply(change);
    }
}

fn open_store(path: &Path) -> Option<ChatStore> {
    ChatStore::open(path)
        .or_else(|error| {
            // The list still works without a cache; it just starts empty
            // next time.
            tracing::error!(%error, path = %path.display(), "could not open the chat cache");
            ChatStore::in_memory()
        })
        .inspect_err(|error| tracing::error!(%error, "could not create an in-memory chat cache"))
        .ok()
}

fn snapshot(book: &ChatBook) -> Snapshot {
    let now = unix_now();
    Snapshot {
        rows: book
            .listed()
            .into_iter()
            .map(|chat| ChatRow::new(chat, now))
            .collect(),
        archived: book.archived_count(),
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}
