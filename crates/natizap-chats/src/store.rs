use std::path::Path;

use rusqlite::{Connection, Row, params};

use natizap_protocol::ChatId;

use crate::book::{Chat, ChatBook, Dirty, Mute, Person, Preview};

/// Schema history: `MIGRATIONS[n]` upgrades a cache from version `n` to
/// `n + 1`. Caches are migrated, never rebuilt: WhatsApp sends the history
/// only once, right after linking, so a dropped cache cannot be refilled.
const MIGRATIONS: &[&str] = &[
    "CREATE TABLE chats (
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
    );",
    // Names move to their own table; chats that only existed because a name
    // arrived for them are dropped.
    "CREATE TABLE people (
        id TEXT PRIMARY KEY,
        contact_name TEXT,
        push_name TEXT
    );
    INSERT INTO people SELECT id, contact_name, push_name FROM chats
        WHERE contact_name IS NOT NULL OR push_name IS NOT NULL;
    DELETE FROM chats WHERE preview_text IS NULL AND active_at = 0;
    ALTER TABLE chats DROP COLUMN contact_name;
    ALTER TABLE chats DROP COLUMN push_name;",
    "ALTER TABLE people ADD COLUMN business_name TEXT;
    ALTER TABLE people ADD COLUMN verified INTEGER NOT NULL DEFAULT 0;",
];

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
        migrate(&connection)?;
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
        let people = self
            .connection
            .prepare("SELECT id, contact_name, push_name, business_name, verified FROM people")?
            .query_map([], |row| {
                Ok((
                    ChatId::new(&row.get::<_, String>(0)?),
                    Person {
                        contact_name: row.get(1)?,
                        push_name: row.get(2)?,
                        business_name: row.get(3)?,
                        verified: row.get(4)?,
                    },
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(ChatBook::restore(chats, aliases, people))
    }

    /// Forgets every chat.
    pub(crate) fn clear(&mut self) -> rusqlite::Result<()> {
        self.connection
            .execute_batch("DELETE FROM chats; DELETE FROM aliases; DELETE FROM people;")
    }

    /// Writes what changed in `book`, in one transaction.
    pub(crate) fn save(&mut self, book: &ChatBook, dirty: &Dirty) -> rusqlite::Result<()> {
        let tx = self.connection.transaction()?;
        {
            let mut upsert = tx.prepare_cached(
                "INSERT OR REPLACE INTO chats (id, subject, preview_text, preview_sent_at,
                 preview_from_me, preview_author, active_at, unread, marked_unread, pinned_at,
                 muted_until, archived)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            )?;
            let mut delete = tx.prepare_cached("DELETE FROM chats WHERE id = ?1")?;
            for id in &dirty.chats {
                match book.get(id).filter(|chat| chat.id == *id) {
                    Some(chat) => {
                        let preview = chat.preview.as_ref();
                        upsert.execute(params![
                            chat.id.as_str(),
                            chat.subject,
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
            let mut person =
                tx.prepare_cached("INSERT OR REPLACE INTO people VALUES (?1, ?2, ?3, ?4, ?5)")?;
            for id in &dirty.people {
                if let Some(names) = book.person_named(id) {
                    person.execute(params![
                        id.as_str(),
                        names.contact_name,
                        names.push_name,
                        names.business_name,
                        names.verified,
                    ])?;
                }
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

/// Brings the cache up to the latest schema, one migration per transaction.
fn migrate(connection: &Connection) -> rusqlite::Result<()> {
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let applied = usize::try_from(version).unwrap_or(0);
    for (from, migration) in MIGRATIONS.iter().enumerate().skip(applied) {
        connection.execute_batch(&format!(
            "BEGIN; {migration} PRAGMA user_version = {}; COMMIT;",
            from + 1
        ))?;
    }
    Ok(())
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
        subject: row.get("subject")?,
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
    fn upgrades_a_version_1_cache_keeping_chats_and_names() -> rusqlite::Result<()> {
        let connection = Connection::open_in_memory()?;
        connection.execute_batch(&format!("{} PRAGMA user_version = 1;", MIGRATIONS[0]))?;
        connection.execute_batch(
            "INSERT INTO chats VALUES
                ('1@lid', 'Ana', NULL, NULL, 'hi', 5, 0, NULL, 5, 0, 0, NULL, NULL, 0),
                ('2@lid', NULL, NULL, 'Bia', NULL, NULL, NULL, NULL, 0, 0, 0, NULL, NULL, 0);",
        )?;

        let store = ChatStore::init(connection)?;
        let book = store.load()?;
        let listed: Vec<&str> = book.listed().iter().map(|chat| chat.id.as_str()).collect();
        assert_eq!(listed, ["1@lid"]);
        let ana = book
            .person_named(&ChatId::new("1@lid"))
            .and_then(|p| p.contact_name.clone());
        assert_eq!(ana.as_deref(), Some("Ana"));
        let bia = book
            .person_named(&ChatId::new("2@lid"))
            .and_then(|p| p.push_name.clone());
        assert_eq!(bia.as_deref(), Some("Bia"));
        Ok(())
    }

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
