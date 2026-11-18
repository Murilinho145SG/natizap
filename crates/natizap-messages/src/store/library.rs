//! Reading everything a chat shared, for its media, docs and links, and
//! how far back to show a chat to reach one of them.

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

    /// How many messages filed under any of `chats` were sent at or after
    /// `sent_at`: how far back the chat must show to reach a message sent
    /// then.
    pub(crate) fn count_since(&self, chats: &[ChatId], sent_at: i64) -> rusqlite::Result<usize> {
        let placeholders = vec!["?"; chats.len()].join(", ");
        let count: i64 = self.connection.query_row(
            &format!(
                "SELECT COUNT(*) FROM messages WHERE chat IN ({placeholders}) AND sent_at >= ?"
            ),
            params_from_iter(
                chats
                    .iter()
                    .map(|chat| rusqlite::types::Value::from(chat.as_str().to_owned()))
                    .chain([rusqlite::types::Value::from(sent_at)]),
            ),
            |row| row.get(0),
        )?;
        Ok(usize::try_from(count).unwrap_or_default())
    }
}
