//! Handing other features what the message store kept for them: status
//! updates and missed calls received before those features kept their own.

use std::path::Path;

use gpui::{App, AppContext as _, Task};
use natizap_protocol::{CallOutcome, CallRecord, ChatId, ContentKind, Media};

use super::MessageLog;
use crate::store::MessageStore;

/// A status update the message store kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredStatus {
    /// Who posted it.
    pub poster: ChatId,
    pub id: String,
    /// When, in Unix seconds.
    pub sent_at: i64,
    pub kind: ContentKind,
    /// Its text or caption.
    pub text: Option<String>,
    /// The name the poster goes by.
    pub author: Option<String>,
    pub media: Option<Media>,
    /// This account posted it.
    pub mine: bool,
}

impl MessageLog {
    /// The status updates the store kept since `since`, oldest first.
    /// Read off the UI thread.
    pub fn stored_statuses(&self, since: i64, cx: &App) -> Task<Vec<StoredStatus>> {
        let path = self.store_path.clone();
        cx.background_spawn(async move {
            read(&path, |store| store.statuses_since(since))
                .into_iter()
                .filter_map(|message| {
                    Some(StoredStatus {
                        poster: message.sender?,
                        id: message.id,
                        sent_at: message.sent_at,
                        kind: message.kind,
                        text: message.text,
                        author: message.author,
                        media: message.media,
                        mine: message.from_me,
                    })
                })
                .collect()
        })
    }

    /// The missed calls the store kept in chats, newest first, as call
    /// history. Read off the UI thread.
    pub fn stored_missed_calls(&self, cx: &App) -> Task<Vec<CallRecord>> {
        let path = self.store_path.clone();
        cx.background_spawn(async move {
            read(&path, MessageStore::missed_calls)
                .into_iter()
                .map(|message| {
                    // In a group the caller is the sender; else the chat.
                    let (peer, group) = match message.sender {
                        Some(caller) => (caller, Some(message.chat)),
                        None => (message.chat, None),
                    };
                    CallRecord {
                        id: message.id,
                        peer,
                        group,
                        incoming: true,
                        video: message.kind == ContentKind::MissedVideoCall,
                        outcome: CallOutcome::Missed,
                        at: message.sent_at,
                        seconds: None,
                    }
                })
                .collect()
        })
    }
}

fn read<T>(path: &Path, query: impl FnOnce(&MessageStore) -> rusqlite::Result<Vec<T>>) -> Vec<T> {
    MessageStore::open(path)
        .and_then(|store| query(&store))
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "could not read what the message store kept");
            Vec::new()
        })
}
