use std::collections::HashMap;
use std::path::Path;

use natizap_protocol::{ChatId, Forwarding, Media, Unread};
use rusqlite::{Connection, params, params_from_iter};

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
    "ALTER TABLE messages ADD COLUMN media_pages INTEGER;",
    // Stills taken before videos were turned upright show phone videos
    // sideways; forgetting them makes them again, upright.
    "UPDATE messages SET media_poster = NULL WHERE kind = 'Video';",
    "ALTER TABLE messages ADD COLUMN link_url TEXT;
     ALTER TABLE messages ADD COLUMN link_title TEXT;
     ALTER TABLE messages ADD COLUMN link_description TEXT;
     ALTER TABLE messages ADD COLUMN link_thumbnail BLOB;",
    // `voter` is empty for this account's own vote; `options` holds one
    // chosen option per line, none when the vote was taken back.
    "ALTER TABLE messages ADD COLUMN secret BLOB;
     CREATE TABLE votes (
        poll_id TEXT NOT NULL,
        voter TEXT NOT NULL,
        options TEXT NOT NULL,
        cast_at INTEGER NOT NULL,
        PRIMARY KEY (poll_id, voter)
    );",
    // A full-text index of every message's text, for search. Trigrams
    // find any run of three or more characters, mid-word and in any
    // language, folding case; the triggers keep it in step with every
    // write, edits and deletions included.
    "CREATE VIRTUAL TABLE messages_fts USING fts5(
        text, content = 'messages', content_rowid = 'rowid', tokenize = 'trigram'
     );
     CREATE TRIGGER messages_fts_insert AFTER INSERT ON messages BEGIN
        INSERT INTO messages_fts (rowid, text) VALUES (new.rowid, new.text);
     END;
     CREATE TRIGGER messages_fts_delete AFTER DELETE ON messages BEGIN
        INSERT INTO messages_fts (messages_fts, rowid, text)
            VALUES ('delete', old.rowid, old.text);
     END;
     CREATE TRIGGER messages_fts_update AFTER UPDATE OF text ON messages BEGIN
        INSERT INTO messages_fts (messages_fts, rowid, text)
            VALUES ('delete', old.rowid, old.text);
        INSERT INTO messages_fts (rowid, text) VALUES (new.rowid, new.text);
     END;
     INSERT INTO messages_fts (messages_fts) VALUES ('rebuild');",
    // Stars arrive with the account state, often before the history that
    // holds their messages: kept here, they reach a message once it is.
    "CREATE TABLE stars (id TEXT PRIMARY KEY, starred INTEGER NOT NULL);
     CREATE TRIGGER messages_star_kept AFTER INSERT ON messages
     WHEN EXISTS (SELECT 1 FROM stars WHERE id = new.id) BEGIN
        UPDATE messages SET starred = (SELECT starred FROM stars WHERE id = new.id)
            WHERE rowid = new.rowid;
     END;",
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
                    tx.prepare_cached("DELETE FROM votes WHERE poll_id = ?1")?
                        .execute(params![id])?;
                }
                MessageChange::Accepted {
                    chat,
                    local_id,
                    id,
                    media,
                    secret,
                } => {
                    tx.prepare_cached(
                        "UPDATE messages SET id = ?3, delivery = 'sent',
                             secret = coalesce(?4, secret)
                         WHERE chat = ?1 AND id = ?2",
                    )?
                    .execute(params![chat.as_str(), local_id, id, secret])?;
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
                MessageChange::MediaSaved {
                    message_id,
                    file,
                    direct_path,
                } => save_media(&tx, message_id, file, direct_path.as_deref())?,
                MessageChange::Pinned { message_id, until } => {
                    tx.prepare_cached("UPDATE messages SET pinned_until = ?2 WHERE id = ?1")?
                        .execute(params![message_id, until])?;
                }
                MessageChange::Starred {
                    message_id,
                    starred,
                } => {
                    star(&tx, message_id, *starred)?;
                }
                MessageChange::PosterSaved { message_id, file } => {
                    save_poster(&tx, message_id, file)?;
                }
                MessageChange::Reacted {
                    message_id,
                    reactor,
                    emoji,
                    ..
                } => react(&tx, message_id, reactor.as_ref(), emoji.as_deref())?,
                MessageChange::Voted {
                    poll_id,
                    voter,
                    options,
                    cast_at,
                } => votes::record(&tx, poll_id, voter.as_ref(), options, *cast_at)?,
                // Opened into `Voted` by `open_votes` before it is kept.
                MessageChange::VoteArrived(_) => {}
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
        self.connection
            .execute_batch("DELETE FROM messages; DELETE FROM reactions; DELETE FROM votes;")
    }
}

/// Keeps `reactor`'s reaction to `message_id` (`None` is this account), or
/// forgets it when `emoji` is empty or `None`.
fn react(
    tx: &rusqlite::Transaction<'_>,
    message_id: &str,
    reactor: Option<&ChatId>,
    emoji: Option<&str>,
) -> rusqlite::Result<()> {
    let reactor = reactor.map_or("", ChatId::as_str);
    match emoji.filter(|emoji| !emoji.is_empty()) {
        Some(emoji) => tx
            .prepare_cached("INSERT OR REPLACE INTO reactions VALUES (?1, ?2, ?3)")?
            .execute(params![message_id, reactor, emoji])?,
        None => tx
            .prepare_cached("DELETE FROM reactions WHERE message_id = ?1 AND reactor = ?2")?
            .execute(params![message_id, reactor])?,
    };
    Ok(())
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

/// Stars or unstars a message, now or once it arrives.
fn star(tx: &rusqlite::Transaction<'_>, message_id: &str, starred: bool) -> rusqlite::Result<()> {
    tx.prepare_cached("UPDATE messages SET starred = ?2 WHERE id = ?1")?
        .execute(params![message_id, starred])?;
    tx.prepare_cached(
        "INSERT INTO stars (id, starred) VALUES (?1, ?2)
         ON CONFLICT (id) DO UPDATE SET starred = excluded.starred",
    )?
    .execute(params![message_id, starred])?;
    Ok(())
}

/// Files a message. A copy already there keeps what it holds, and only
/// gains the media details it lacks: messages filed before natizap read
/// media come again when the phone resends their history.
fn insert(tx: &rusqlite::Transaction<'_>, message: &Message) -> rusqlite::Result<()> {
    let quote = message.quote.as_ref();
    let media = message.media.as_ref();
    let hd = media.and_then(|media| media.hd_thumbnail.as_ref());
    let link = message.link.as_ref();
    tx.prepare_cached(
        "INSERT INTO messages (chat, id, from_me, sender, author, sent_at,
             kind, text, edited, deleted, delivery, quoted_id, quoted_sender,
             quoted_summary, media_mime, media_name, media_size, media_width,
             media_height, media_seconds, media_thumbnail, media_direct_path,
             media_key, media_sha256, media_enc_sha256, media_file, media_waveform,
             media_hd_thumbnail_path, media_hd_thumbnail_sha256,
             media_hd_thumbnail_enc_sha256, sender_phone, extra, forwarding, media_pages,
             link_url, link_title, link_description, link_thumbnail, secret)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
             ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28,
             ?29, ?30, ?31, ?32, ?33, ?34, ?35, ?36, ?37, ?38, ?39)
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
                 forwarding = coalesce(forwarding, excluded.forwarding),
                 media_pages = coalesce(media_pages, excluded.media_pages),
                 link_url = coalesce(link_url, excluded.link_url),
                 link_title = coalesce(link_title, excluded.link_title),
                 link_description = coalesce(link_description, excluded.link_description),
                 link_thumbnail = coalesce(link_thumbnail, excluded.link_thumbnail),
                 secret = coalesce(secret, excluded.secret)",
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
        media.and_then(|media| media.pages),
        link.and_then(|link| link.url.as_ref()),
        link.and_then(|link| link.title.as_ref()),
        link.and_then(|link| link.description.as_ref()),
        link.and_then(|link| link.thumbnail.as_ref()),
        message.secret,
    ])?;

    Ok(())
}

mod extra;
mod votes;

pub(crate) use votes::Vote;
mod backlog;
mod library;
mod rows;
mod search;

use rows::message_from_row;

/// Records where a still of `message_id`'s video was saved.
fn save_poster(
    tx: &rusqlite::Transaction<'_>,
    message_id: &str,
    file: &Path,
) -> rusqlite::Result<()> {
    tx.prepare_cached("UPDATE messages SET media_poster = ?2 WHERE id = ?1")?
        .execute(params![message_id, file.to_string_lossy()])
        .map(drop)
}

/// Records where `message_id`'s attachment was downloaded to, and the link
/// it now lives at when it had to be uploaded again.
fn save_media(
    tx: &rusqlite::Transaction<'_>,
    message_id: &str,
    file: &Path,
    direct_path: Option<&str>,
) -> rusqlite::Result<()> {
    tx.prepare_cached(
        "UPDATE messages SET media_file = ?2, \
         media_direct_path = COALESCE(?3, media_direct_path) WHERE id = ?1",
    )?
    .execute(params![message_id, file.to_string_lossy(), direct_path])
    .map(drop)
}

#[cfg(test)]
mod tests;
