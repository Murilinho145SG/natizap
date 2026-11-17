use std::collections::HashMap;
use std::path::{Path, PathBuf};

use natizap_protocol::{ChatId, ContentKind, HdThumbnail, Media, Quote, Unread};
use rusqlite::{Connection, Row, params, params_from_iter};

use crate::message::{Delivery, Message, MessageChange, Reaction};

/// Schema history: `MIGRATIONS[n]` upgrades a store from version `n` to
/// `n + 1`. Stores are migrated, never rebuilt: WhatsApp sends the history
/// only once, right after linking, so a dropped store cannot be refilled.
const MIGRATIONS: &[&str] = &[
    "CREATE TABLE messages (
        chat TEXT NOT NULL,
        id TEXT NOT NULL,
        from_me INTEGER NOT NULL,
        sender TEXT,
        author TEXT,
        sent_at INTEGER NOT NULL,
        kind TEXT NOT NULL,
        text TEXT,
        edited INTEGER NOT NULL,
        deleted INTEGER NOT NULL,
        delivery TEXT NOT NULL,
        PRIMARY KEY (chat, id)
    );
    CREATE INDEX messages_by_time ON messages (chat, sent_at);",
    "ALTER TABLE messages ADD COLUMN quoted_id TEXT;
    ALTER TABLE messages ADD COLUMN quoted_sender TEXT;
    ALTER TABLE messages ADD COLUMN quoted_summary TEXT;",
    // `reactor` is empty for this account's own reaction.
    "CREATE TABLE reactions (
        message_id TEXT NOT NULL,
        reactor TEXT NOT NULL,
        emoji TEXT NOT NULL,
        PRIMARY KEY (message_id, reactor)
    );",
    "ALTER TABLE messages ADD COLUMN media_mime TEXT;
    ALTER TABLE messages ADD COLUMN media_name TEXT;
    ALTER TABLE messages ADD COLUMN media_size INTEGER;
    ALTER TABLE messages ADD COLUMN media_width INTEGER;
    ALTER TABLE messages ADD COLUMN media_height INTEGER;
    ALTER TABLE messages ADD COLUMN media_seconds INTEGER;
    ALTER TABLE messages ADD COLUMN media_thumbnail BLOB;
    ALTER TABLE messages ADD COLUMN media_direct_path TEXT;
    ALTER TABLE messages ADD COLUMN media_key BLOB;
    ALTER TABLE messages ADD COLUMN media_sha256 BLOB;
    ALTER TABLE messages ADD COLUMN media_enc_sha256 BLOB;
    ALTER TABLE messages ADD COLUMN media_file TEXT;",
    "ALTER TABLE messages ADD COLUMN media_waveform BLOB;",
    "ALTER TABLE messages ADD COLUMN media_poster TEXT;",
    "ALTER TABLE messages ADD COLUMN media_hd_thumbnail_path TEXT;
    ALTER TABLE messages ADD COLUMN media_hd_thumbnail_sha256 BLOB;
    ALTER TABLE messages ADD COLUMN media_hd_thumbnail_enc_sha256 BLOB;",
];

/// Every message natizap has seen, on disk.
#[derive(Debug)]
pub(crate) struct MessageStore {
    connection: Connection,
}

impl MessageStore {
    /// Opens the store at `path`, creating it if needed.
    pub(crate) fn open(path: &Path) -> rusqlite::Result<Self> {
        Self::init(Connection::open(path)?)
    }

    /// A store that lives only as long as the process.
    pub(crate) fn in_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(connection: Connection) -> rusqlite::Result<Self> {
        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let applied = usize::try_from(version).unwrap_or(0);
        for (from, migration) in MIGRATIONS.iter().enumerate().skip(applied) {
            connection.execute_batch(&format!(
                "BEGIN; {migration} PRAGMA user_version = {}; COMMIT;",
                from + 1
            ))?;
        }
        connection.pragma_update(None, "journal_mode", "WAL")?;
        Ok(Self { connection })
    }

    /// Applies a batch of changes in one transaction.
    pub(crate) fn apply(&mut self, changes: &[MessageChange]) -> rusqlite::Result<()> {
        let tx = self.connection.transaction()?;
        for change in changes {
            match change {
                // History and live delivery can repeat a message; the first
                // copy wins, and edits and deletions are applied on top.
                MessageChange::Received(message) | MessageChange::Sending(message) => {
                    insert(&tx, message)?;
                }
                MessageChange::Edited { chat, id, text } => {
                    tx.prepare_cached(
                        "UPDATE messages SET text = ?3, edited = 1 WHERE chat = ?1 AND id = ?2",
                    )?
                    .execute(params![chat.as_str(), id, text])?;
                }
                MessageChange::Deleted { chat, id } => {
                    tx.prepare_cached(
                        "UPDATE messages SET text = NULL, deleted = 1 WHERE chat = ?1 AND id = ?2",
                    )?
                    .execute(params![chat.as_str(), id])?;
                }
                MessageChange::Accepted {
                    chat,
                    local_id,
                    id,
                    media,
                } => {
                    tx.prepare_cached(
                        "UPDATE messages SET id = ?3, delivery = 'sent'
                         WHERE chat = ?1 AND id = ?2",
                    )?
                    .execute(params![chat.as_str(), local_id, id])?;
                    if let Some(media) = media {
                        // Where the file went: what this account's other
                        // devices, and a later download, need.
                        tx.prepare_cached(
                            "UPDATE messages SET media_direct_path = ?3, media_key = ?4,
                             media_sha256 = ?5, media_enc_sha256 = ?6, media_size = ?7
                             WHERE chat = ?1 AND id = ?2",
                        )?
                        .execute(params![
                            chat.as_str(),
                            id,
                            media.direct_path,
                            media.media_key,
                            media.file_sha256,
                            media.file_enc_sha256,
                            media.size.and_then(|size| i64::try_from(size).ok()),
                        ])?;
                    }
                }
                MessageChange::Progressed { ids, delivery } => {
                    // Receipts arrive out of order; a message only moves
                    // forward.
                    let behind: Vec<&str> = Delivery::ALL
                        .into_iter()
                        .filter(|state| (Delivery::Sending..*delivery).contains(state))
                        .map(Delivery::name)
                        .collect();
                    let mut progress = tx.prepare_cached(&format!(
                        "UPDATE messages SET delivery = ?1
                         WHERE id = ?2 AND from_me = 1 AND delivery IN ('{}')",
                        behind.join("', '")
                    ))?;
                    for id in ids {
                        progress.execute(params![delivery.name(), id])?;
                    }
                }
                MessageChange::MediaSaved { message_id, file } => {
                    tx.prepare_cached("UPDATE messages SET media_file = ?2 WHERE id = ?1")?
                        .execute(params![message_id, file.to_string_lossy()])?;
                }
                MessageChange::PosterSaved { message_id, file } => {
                    tx.prepare_cached("UPDATE messages SET media_poster = ?2 WHERE id = ?1")?
                        .execute(params![message_id, file.to_string_lossy()])?;
                }
                MessageChange::Reacted {
                    message_id,
                    reactor,
                    emoji,
                    ..
                } => {
                    let reactor = reactor.as_ref().map_or("", ChatId::as_str);
                    match emoji.as_deref().filter(|emoji| !emoji.is_empty()) {
                        Some(emoji) => tx
                            .prepare_cached("INSERT OR REPLACE INTO reactions VALUES (?1, ?2, ?3)")?
                            .execute(params![message_id, reactor, emoji])?,
                        None => tx
                            .prepare_cached(
                                "DELETE FROM reactions WHERE message_id = ?1 AND reactor = ?2",
                            )?
                            .execute(params![message_id, reactor])?,
                    };
                }
                MessageChange::Rejected { chat, local_id } => {
                    tx.prepare_cached(
                        "UPDATE messages SET delivery = 'failed' WHERE chat = ?1 AND id = ?2",
                    )?
                    .execute(params![chat.as_str(), local_id])?;
                }
            }
        }
        tx.commit()
    }

    /// The newest `limit` messages filed under any of `chats`, oldest first.
    pub(crate) fn latest(&self, chats: &[ChatId], limit: usize) -> rusqlite::Result<Vec<Message>> {
        let placeholders = vec!["?"; chats.len()].join(", ");
        let mut statement = self.connection.prepare(&format!(
            "SELECT * FROM messages WHERE chat IN ({placeholders})
             ORDER BY sent_at DESC, rowid DESC LIMIT {limit}"
        ))?;
        let mut messages = statement
            .query_map(
                params_from_iter(chats.iter().map(ChatId::as_str)),
                message_from_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        messages.reverse();
        Ok(messages)
    }

    /// The reactions on each of `messages`, by message id.
    pub(crate) fn reactions(
        &self,
        messages: &[Message],
    ) -> rusqlite::Result<HashMap<String, Vec<Reaction>>> {
        let placeholders = vec!["?"; messages.len()].join(", ");
        let mut statement = self.connection.prepare(&format!(
            "SELECT message_id, reactor, emoji FROM reactions WHERE message_id IN ({placeholders})"
        ))?;
        let rows = statement.query_map(
            params_from_iter(messages.iter().map(|message| message.id.as_str())),
            |row| {
                let reactor: String = row.get(1)?;
                Ok((
                    row.get::<_, String>(0)?,
                    Reaction {
                        emoji: row.get(2)?,
                        mine: reactor.is_empty(),
                    },
                ))
            },
        )?;
        let mut reactions: HashMap<String, Vec<Reaction>> = HashMap::new();
        for row in rows {
            let (id, reaction) = row?;
            reactions.entry(id).or_default().push(reaction);
        }
        Ok(reactions)
    }

    /// The newest `count` messages others sent in `chats`, to acknowledge as
    /// read.
    pub(crate) fn latest_incoming(
        &self,
        chats: &[ChatId],
        count: u32,
    ) -> rusqlite::Result<Vec<Unread>> {
        let placeholders = vec!["?"; chats.len()].join(", ");
        let mut statement = self.connection.prepare(&format!(
            "SELECT id, sender FROM messages WHERE chat IN ({placeholders}) AND from_me = 0
             ORDER BY sent_at DESC LIMIT {count}"
        ))?;
        statement
            .query_map(params_from_iter(chats.iter().map(ChatId::as_str)), |row| {
                Ok(Unread {
                    id: row.get(0)?,
                    sender: row
                        .get::<_, Option<String>>(1)?
                        .map(|jid| ChatId::new(&jid)),
                })
            })?
            .collect()
    }

    /// Forgets every message.
    pub(crate) fn clear(&mut self) -> rusqlite::Result<()> {
        self.connection.execute_batch("DELETE FROM messages;")
    }
}

/// Files a message unless a copy is already there.
fn insert(tx: &rusqlite::Transaction<'_>, message: &Message) -> rusqlite::Result<()> {
    let quote = message.quote.as_ref();
    let media = message.media.as_ref();
    let hd = media.and_then(|media| media.hd_thumbnail.as_ref());
    tx.prepare_cached(
        "INSERT OR IGNORE INTO messages (chat, id, from_me, sender, author, sent_at,
             kind, text, edited, deleted, delivery, quoted_id, quoted_sender,
             quoted_summary, media_mime, media_name, media_size, media_width,
             media_height, media_seconds, media_thumbnail, media_direct_path,
             media_key, media_sha256, media_enc_sha256, media_file, media_waveform,
             media_hd_thumbnail_path, media_hd_thumbnail_sha256,
             media_hd_thumbnail_enc_sha256)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
             ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28,
             ?29, ?30)",
    )?
    .execute(params![
        message.chat.as_str(),
        message.id,
        message.from_me,
        message.sender.as_ref().map(ChatId::as_str),
        message.author,
        message.sent_at,
        message.kind.label(),
        message.text,
        message.edited,
        message.deleted,
        message.delivery.name(),
        quote.map(|quote| &quote.id),
        quote
            .and_then(|quote| quote.sender.as_ref())
            .map(ChatId::as_str),
        quote.and_then(|quote| quote.summary.as_ref()),
        media.and_then(|media| media.mime.as_ref()),
        media.and_then(|media| media.file_name.as_ref()),
        media
            .and_then(|media| media.size)
            .and_then(|size| i64::try_from(size).ok()),
        media.and_then(|media| media.width),
        media.and_then(|media| media.height),
        media.and_then(|media| media.seconds),
        media.and_then(|media| media.thumbnail.as_ref()),
        media.map(|media| &media.direct_path),
        media.map(|media| &media.media_key),
        media.map(|media| &media.file_sha256),
        media.map(|media| &media.file_enc_sha256),
        message
            .media_file
            .as_ref()
            .map(|file| file.to_string_lossy()),
        media.and_then(|media| media.waveform.as_ref()),
        hd.map(|hd| &hd.direct_path),
        hd.map(|hd| &hd.sha256),
        hd.map(|hd| &hd.enc_sha256),
    ])?;

    Ok(())
}

fn media_from_row(row: &Row<'_>) -> rusqlite::Result<Option<Media>> {
    let Some(direct_path) = row.get::<_, Option<String>>("media_direct_path")? else {
        return Ok(None);
    };
    Ok(Some(Media {
        mime: row.get("media_mime")?,
        file_name: row.get("media_name")?,
        size: row
            .get::<_, Option<i64>>("media_size")?
            .and_then(|size| u64::try_from(size).ok()),
        width: row.get("media_width")?,
        height: row.get("media_height")?,
        seconds: row.get("media_seconds")?,
        thumbnail: row.get("media_thumbnail")?,
        waveform: row.get("media_waveform")?,
        hd_thumbnail: hd_thumbnail_from_row(row)?,
        direct_path,
        media_key: row
            .get::<_, Option<Vec<u8>>>("media_key")?
            .unwrap_or_default(),
        file_sha256: row
            .get::<_, Option<Vec<u8>>>("media_sha256")?
            .unwrap_or_default(),
        file_enc_sha256: row
            .get::<_, Option<Vec<u8>>>("media_enc_sha256")?
            .unwrap_or_default(),
    }))
}

fn hd_thumbnail_from_row(row: &Row<'_>) -> rusqlite::Result<Option<HdThumbnail>> {
    let Some(direct_path) = row.get::<_, Option<String>>("media_hd_thumbnail_path")? else {
        return Ok(None);
    };
    Ok(Some(HdThumbnail {
        direct_path,
        sha256: row
            .get::<_, Option<Vec<u8>>>("media_hd_thumbnail_sha256")?
            .unwrap_or_default(),
        enc_sha256: row
            .get::<_, Option<Vec<u8>>>("media_hd_thumbnail_enc_sha256")?
            .unwrap_or_default(),
    }))
}

fn quote_from_row(row: &Row<'_>) -> rusqlite::Result<Option<Quote>> {
    let Some(id) = row.get::<_, Option<String>>("quoted_id")? else {
        return Ok(None);
    };
    let sender: Option<String> = row.get("quoted_sender")?;
    Ok(Some(Quote {
        id,
        sender: sender.map(|jid| ChatId::new(&jid)),
        summary: row.get("quoted_summary")?,
    }))
}

fn message_from_row(row: &Row<'_>) -> rusqlite::Result<Message> {
    let kind: String = row.get("kind")?;
    let delivery: String = row.get("delivery")?;
    Ok(Message {
        chat: ChatId::new(&row.get::<_, String>("chat")?),
        id: row.get("id")?,
        from_me: row.get("from_me")?,
        sender: row
            .get::<_, Option<String>>("sender")?
            .map(|jid| ChatId::new(&jid)),
        author: row.get("author")?,
        sent_at: row.get("sent_at")?,
        kind: ContentKind::from_label(&kind).unwrap_or(ContentKind::Text),
        text: row.get("text")?,
        edited: row.get("edited")?,
        deleted: row.get("deleted")?,
        delivery: Delivery::from_name(&delivery).unwrap_or(Delivery::Sent),
        quote: quote_from_row(row)?,
        media: media_from_row(row)?,
        media_file: row
            .get::<_, Option<String>>("media_file")?
            .map(PathBuf::from),
        media_poster: row
            .get::<_, Option<String>>("media_poster")?
            .map(PathBuf::from),
    })
}

#[cfg(test)]
mod tests;
