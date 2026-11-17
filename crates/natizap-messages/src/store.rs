use std::collections::HashMap;
use std::path::{Path, PathBuf};

use natizap_protocol::{ChatId, ContentKind, Forwarding, HdThumbnail, Media, Quote, Unread};
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
    "ALTER TABLE messages ADD COLUMN starred INTEGER NOT NULL DEFAULT 0;",
    "ALTER TABLE messages ADD COLUMN pinned_until INTEGER;",
    "ALTER TABLE messages ADD COLUMN sender_phone TEXT;",
    "ALTER TABLE messages ADD COLUMN extra TEXT;",
    "ALTER TABLE messages ADD COLUMN forwarding INTEGER;",
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
                MessageChange::Removed { chat, id } => {
                    tx.prepare_cached("DELETE FROM messages WHERE chat = ?1 AND id = ?2")?
                        .execute(params![chat.as_str(), id])?;
                    tx.prepare_cached("DELETE FROM reactions WHERE message_id = ?1")?
                        .execute(params![id])?;
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
                        record_upload(&tx, chat, id, media)?;
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
                MessageChange::Pinned { message_id, until } => {
                    tx.prepare_cached("UPDATE messages SET pinned_until = ?2 WHERE id = ?1")?
                        .execute(params![message_id, until])?;
                }
                MessageChange::Starred {
                    message_id,
                    starred,
                } => {
                    tx.prepare_cached("UPDATE messages SET starred = ?2 WHERE id = ?1")?
                        .execute(params![message_id, starred])?;
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

    /// How many photos, videos and documents `chats` hold, and the newest
    /// `recent` photos and videos, newest first.
    pub(crate) fn media(
        &self,
        chats: &[ChatId],
        recent: usize,
    ) -> rusqlite::Result<(usize, Vec<Message>)> {
        let placeholders = vec!["?"; chats.len()].join(", ");
        let addresses = || params_from_iter(chats.iter().map(ChatId::as_str));
        let count: i64 = self.connection.query_row(
            &format!(
                "SELECT COUNT(*) FROM messages WHERE chat IN ({placeholders})
                 AND deleted = 0 AND kind IN ('Photo', 'Video', 'Document')"
            ),
            addresses(),
            |row| row.get(0),
        )?;
        let newest = self
            .connection
            .prepare(&format!(
                "SELECT * FROM messages WHERE chat IN ({placeholders})
                 AND deleted = 0 AND kind IN ('Photo', 'Video')
                 ORDER BY sent_at DESC LIMIT {recent}"
            ))?
            .query_map(addresses(), message_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok((usize::try_from(count).unwrap_or_default(), newest))
    }

    /// How many messages in `chats` this account starred.
    pub(crate) fn starred_count(&self, chats: &[ChatId]) -> rusqlite::Result<usize> {
        let placeholders = vec!["?"; chats.len()].join(", ");
        let count: i64 = self.connection.query_row(
            &format!(
                "SELECT COUNT(*) FROM messages WHERE chat IN ({placeholders}) AND starred = 1"
            ),
            params_from_iter(chats.iter().map(ChatId::as_str)),
            |row| row.get(0),
        )?;
        Ok(usize::try_from(count).unwrap_or_default())
    }

    /// The message pinned in any of `chats` that stays pinned longest past
    /// `now` (Unix seconds), if any.
    pub(crate) fn pinned(&self, chats: &[ChatId], now: i64) -> rusqlite::Result<Option<Message>> {
        let placeholders = vec!["?"; chats.len()].join(", ");
        let mut statement = self.connection.prepare(&format!(
            "SELECT * FROM messages WHERE chat IN ({placeholders}) AND pinned_until > ?
             ORDER BY pinned_until DESC LIMIT 1"
        ))?;
        let mut values: Vec<rusqlite::types::Value> = chats
            .iter()
            .map(|chat| chat.as_str().to_owned().into())
            .collect();
        values.push(now.into());
        statement
            .query_map(params_from_iter(values), message_from_row)?
            .next()
            .transpose()
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

/// Where a sent file went: what this account's other devices, and a later
/// download, need.
fn record_upload(
    tx: &rusqlite::Transaction<'_>,
    chat: &ChatId,
    id: &str,
    media: &Media,
) -> rusqlite::Result<()> {
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
    Ok(())
}

/// Files a message. A copy already there keeps what it holds, and only
/// gains the media details it lacks: messages filed before natizap read
/// media come again when the phone resends their history.
fn insert(tx: &rusqlite::Transaction<'_>, message: &Message) -> rusqlite::Result<()> {
    let quote = message.quote.as_ref();
    let media = message.media.as_ref();
    let hd = media.and_then(|media| media.hd_thumbnail.as_ref());
    tx.prepare_cached(
        "INSERT INTO messages (chat, id, from_me, sender, author, sent_at,
             kind, text, edited, deleted, delivery, quoted_id, quoted_sender,
             quoted_summary, media_mime, media_name, media_size, media_width,
             media_height, media_seconds, media_thumbnail, media_direct_path,
             media_key, media_sha256, media_enc_sha256, media_file, media_waveform,
             media_hd_thumbnail_path, media_hd_thumbnail_sha256,
             media_hd_thumbnail_enc_sha256, sender_phone, extra, forwarding)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
             ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28,
             ?29, ?30, ?31, ?32, ?33)
             ON CONFLICT (chat, id) DO UPDATE SET
                 media_mime = coalesce(media_mime, excluded.media_mime),
                 media_name = coalesce(media_name, excluded.media_name),
                 media_size = coalesce(media_size, excluded.media_size),
                 media_width = coalesce(media_width, excluded.media_width),
                 media_height = coalesce(media_height, excluded.media_height),
                 media_seconds = coalesce(media_seconds, excluded.media_seconds),
                 media_thumbnail = coalesce(media_thumbnail, excluded.media_thumbnail),
                 media_direct_path = coalesce(media_direct_path, excluded.media_direct_path),
                 media_key = coalesce(media_key, excluded.media_key),
                 media_sha256 = coalesce(media_sha256, excluded.media_sha256),
                 media_enc_sha256 = coalesce(media_enc_sha256, excluded.media_enc_sha256),
                 media_waveform = coalesce(media_waveform, excluded.media_waveform),
                 media_hd_thumbnail_path = coalesce(media_hd_thumbnail_path, excluded.media_hd_thumbnail_path),
                 media_hd_thumbnail_sha256 = coalesce(media_hd_thumbnail_sha256, excluded.media_hd_thumbnail_sha256),
                 media_hd_thumbnail_enc_sha256 = coalesce(media_hd_thumbnail_enc_sha256, excluded.media_hd_thumbnail_enc_sha256),
                 sender_phone = coalesce(sender_phone, excluded.sender_phone),
                 extra = coalesce(extra, excluded.extra),
                 forwarding = coalesce(forwarding, excluded.forwarding)",
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
        message.sender_phone.as_ref().map(ChatId::as_str),
        message.extra.as_ref().map(extra::encode),
        message.forwarding.map(|forwarding| match forwarding {
            Forwarding::Once => 1,
            Forwarding::Often => 2,
        }),
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
        starred: row.get("starred")?,
        sender_phone: row
            .get::<_, Option<String>>("sender_phone")?
            .map(|phone| ChatId::new(&phone)),
        extra: row
            .get::<_, Option<String>>("extra")?
            .as_deref()
            .and_then(extra::decode),
        forwarding: row.get::<_, Option<u8>>("forwarding")?.and_then(
            |forwarding| match forwarding {
                1 => Some(Forwarding::Once),
                2 => Some(Forwarding::Often),
                _ => None,
            },
        ),
    })
}

mod extra;
#[cfg(test)]
mod tests;
