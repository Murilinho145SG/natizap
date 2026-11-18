//! Reading everything a chat shared, for its media, docs and links.

use natizap_protocol::ChatId;
use rusqlite::params_from_iter;

use super::{MessageStore, message_from_row};
use crate::message::Message;

impl MessageStore {
    /// Every photo, video, document and audio file in `chats`, and every
    /// message whose text may hold a link, newest first.
    pub(crate) fn library(&self, chats: &[ChatId]) -> rusqlite::Result<Vec<Message>> {
        let placeholders = vec!["?"; chats.len()].join(", ");
        self.connection
            .prepare(&format!(
                "SELECT * FROM messages WHERE chat IN ({placeholders}) AND deleted = 0
                 AND (kind IN ('Photo', 'Video', 'Document', 'Audio') OR text LIKE '%.%')
                 ORDER BY sent_at DESC"
            ))?
            .query_map(
                params_from_iter(chats.iter().map(ChatId::as_str)),
                message_from_row,
            )?
            .collect()
    }
}
