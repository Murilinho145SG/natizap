//! What the message store kept that other features show too: status
//! updates and missed calls, stored as messages before those features had
//! a store of their own.

use rusqlite::params;

use super::{MessageStore, message_from_row};
use crate::message::Message;

impl MessageStore {
    /// The status updates received since `since`, oldest first.
    pub(crate) fn statuses_since(&self, since: i64) -> rusqlite::Result<Vec<Message>> {
        self.connection
            .prepare(
                "SELECT * FROM messages WHERE chat = 'status@broadcast' AND deleted = 0
                 AND sent_at >= ?1 ORDER BY sent_at",
            )?
            .query_map(params![since], message_from_row)?
            .collect()
    }

    /// Every missed call kept in a chat, newest first.
    pub(crate) fn missed_calls(&self) -> rusqlite::Result<Vec<Message>> {
        self.connection
            .prepare(
                "SELECT * FROM messages WHERE kind IN ('Missed voice call', 'Missed video call')
                 ORDER BY sent_at DESC",
            )?
            .query_map([], message_from_row)?
            .collect()
    }
}
