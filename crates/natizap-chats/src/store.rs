use std::path::Path;

use rusqlite::{Connection, Row, params};

use natizap_protocol::ChatId;

use crate::book::{Chat, ChatBook, Dirty, Mute, Preview};

/// Bumped when the schema changes; older caches are rebuilt from WhatsApp.
const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
    CREATE TABLE chats (
        id TEXT PRIMARY KEY,
        contact_name TEXT,
        subject TEXT,
        push_name TEXT,
        preview_text TEXT,
        preview_sent_at INTEGER,
        preview_from_me INTEGER,
        preview_author TEXT,
        active_at INTEGER NOT NULL,
        unread INTEGER NOT NULL,
        marked_unread INTEGER NOT NULL,
        pinned_at INTEGER,
        muted_until INTEGER,
        archived INTEGER NOT NULL
    );
    CREATE TABLE aliases (
        alias TEXT PRIMARY KEY,
        chat TEXT NOT NULL
    );
";

/// `muted_until` value for [`Mute::Forever`].
const MUTED_FOREVER: i64 = -1;

/// The on-disk cache of the chat list, so it opens instantly and offline.
#[derive(Debug)]
pub(crate) struct ChatStore {
    connection: Connection,
}

impl ChatStore {
    /// Opens the cache at `path`, creating it if needed.
    pub(crate) fn open(path: &Path) -> rusqlite::Result<Self> {
        Self::init(Connection::open(path)?)
    }

    /// A cache that lives only as long as the process.
    pub(crate) fn in_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(connection: Connection) -> rusqlite::Result<Self> {
        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version != SCHEMA_VERSION {
            // The cache only mirrors what WhatsApp syncs again, so an
            // outdated one is dropped instead of migrated.
            connection.execute_batch(&format!(
                "DROP TABLE IF EXISTS chats; DROP TABLE IF EXISTS aliases; {SCHEMA}
                 PRAGMA user_version = {SCHEMA_VERSION};"
            ))?;
        }
        connection.pragma_update(None, "journal_mode", "WAL")?;
        Ok(Self { connection })
    }

    /// Reads the whole cache.
    pub(crate) fn load(&self) -> rusqlite::Result<ChatBook> {
        let chats = self
            .connection
            .prepare("SELECT * FROM chats")?
            .query_map([], chat_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let aliases = self
            .connection
            .prepare("SELECT alias, chat FROM aliases")?
            .query_map([], |row| {
                Ok((
                    ChatId::new(&row.get::<_, String>(0)?),
                    ChatId::new(&row.get::<_, String>(1)?),
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(ChatBook::restore(chats, aliases))
    }

    /// Forgets every chat.
    pub(crate) fn clear(&mut self) -> rusqlite::Result<()> {
        self.connection
            .execute_batch("DELETE FROM chats; DELETE FROM aliases;")
    }

    /// Writes what changed in `book`, in one transaction.
    pub(crate) fn save(&mut self, book: &ChatBook, dirty: &Dirty) -> rusqlite::Result<()> {
        let tx = self.connection.transaction()?;
        {
            let mut upsert = tx.prepare_cached(
                "INSERT OR REPLACE INTO chats VALUES
                 (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            )?;
            let mut delete = tx.prepare_cached("DELETE FROM chats WHERE id = ?1")?;
            for id in &dirty.chats {
                match book.get(id).filter(|chat| chat.id == *id) {
                    Some(chat) => {
                        let preview = chat.preview.as_ref();
                        upsert.execute(params![
                            chat.id.as_str(),
                            chat.contact_name,
                            chat.subject,
                            chat.push_name,
                            preview.map(|p| &p.text),
                            preview.map(|p| p.sent_at),
                            preview.map(|p| p.from_me),
                            preview.and_then(|p| p.author.as_ref()),
                            chat.active_at,
                            chat.unread,
                            chat.marked_unread,
                            chat.pinned_at,
                            chat.mute.map(|mute| match mute {
                                Mute::Forever => MUTED_FOREVER,
                                Mute::Until(until) => until,
                            }),
                            chat.archived,
                        ])?;
                    }
                    None => {
                        delete.execute(params![id.as_str()])?;
                    }
                }
            }
            let mut alias = tx.prepare_cached("INSERT OR REPLACE INTO aliases VALUES (?1, ?2)")?;
            for (from, to) in &dirty.aliases {
                alias.execute(params![from.as_str(), to.as_str()])?;
            }
        }
        tx.commit()
    }

    #[cfg(test)]
    fn chat_count(&self) -> rusqlite::Result<i64> {
        self.connection
            .query_row("SELECT COUNT(*) FROM chats", [], |row| row.get(0))
    }
}

fn chat_from_row(row: &Row<'_>) -> rusqlite::Result<Chat> {
    let preview = match row.get::<_, Option<String>>("preview_text")? {
        Some(text) => Some(Preview {
            text,
            sent_at: row.get("preview_sent_at")?,
            from_me: row.get("preview_from_me")?,
            author: row.get("preview_author")?,
        }),
        None => None,
    };
    Ok(Chat {
        id: ChatId::new(&row.get::<_, String>("id")?),
        contact_name: row.get("contact_name")?,
        subject: row.get("subject")?,
        push_name: row.get("push_name")?,
        preview,
        active_at: row.get("active_at")?,
        unread: row.get("unread")?,
        marked_unread: row.get("marked_unread")?,
        pinned_at: row.get("pinned_at")?,
        mute: row.get::<_, Option<i64>>("muted_until")?.map(|until| {
            if until == MUTED_FOREVER {
                Mute::Forever
            } else {
                Mute::Until(until)
            }
        }),
        archived: row.get("archived")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::ChatChange;

    #[test]
    fn round_trips_chats_aliases_and_deletions() -> rusqlite::Result<()> {
        let mut store = ChatStore::in_memory()?;
        let mut book = ChatBook::default();
        let lid = ChatId::new("1@lid");
        let phone = ChatId::new("1@s.whatsapp.net");
        let preview = Preview {
            text: "hi".into(),
            sent_at: 7,
            from_me: false,
            author: None,
        };
        book.apply(ChatChange::Message(lid.clone(), preview.clone()));
        book.apply(ChatChange::SameChat(lid.clone(), phone.clone()));
        book.apply(ChatChange::Muted(lid.clone(), Some(Mute::Forever)));
        book.apply(ChatChange::Pinned(lid.clone(), Some(3)));
        book.apply(ChatChange::Message(ChatId::new("2@g.us"), preview));
        book.apply(ChatChange::Deleted(ChatId::new("2@g.us")));
        let dirty = book.take_dirty();
        store.save(&book, &dirty)?;

        let restored = store.load()?;
        assert_eq!(restored.get(&phone), book.get(&lid));
        assert_eq!(store.chat_count()?, 1);

        store.clear()?;
        assert_eq!(store.load()?.listed().len(), 0);
        Ok(())
    }
}
