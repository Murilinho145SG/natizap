use std::collections::HashMap;
use std::path::Path;

use natizap_protocol::{ChatId, ContentKind, Quote, Unread};
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
                    let quote = message.quote.as_ref();
                    tx.prepare_cached(
                        "INSERT OR IGNORE INTO messages (chat, id, from_me, sender, author, sent_at,
                         kind, text, edited, deleted, delivery, quoted_id, quoted_sender,
                         quoted_summary)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
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
                        quote.and_then(|quote| quote.sender.as_ref()).map(ChatId::as_str),
                        quote.and_then(|quote| quote.summary.as_ref()),
                    ])?;
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
                MessageChange::Accepted { chat, local_id, id } => {
                    tx.prepare_cached(
                        "UPDATE messages SET id = ?3, delivery = 'sent'
                         WHERE chat = ?1 AND id = ?2",
                    )?
                    .execute(params![chat.as_str(), local_id, id])?;
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(chat: &str, id: &str, sent_at: i64, text: &str) -> Message {
        Message {
            chat: ChatId::new(chat),
            id: id.into(),
            from_me: false,
            sender: None,
            author: None,
            sent_at,
            kind: ContentKind::Text,
            text: Some(text.into()),
            edited: false,
            deleted: false,
            delivery: Delivery::Sent,
            quote: None,
        }
    }

    fn texts(messages: &[Message]) -> Vec<Option<&str>> {
        messages.iter().map(|m| m.text.as_deref()).collect()
    }

    #[test]
    fn returns_the_newest_messages_of_every_address_oldest_first() -> rusqlite::Result<()> {
        let mut store = MessageStore::in_memory()?;
        store.apply(&[
            MessageChange::Received(message("1@lid", "a", 10, "old")),
            MessageChange::Received(message("1@s.whatsapp.net", "b", 20, "middle")),
            MessageChange::Received(message("1@lid", "c", 30, "new")),
            MessageChange::Received(message("2@lid", "d", 40, "other chat")),
        ])?;

        let chats = [ChatId::new("1@lid"), ChatId::new("1@s.whatsapp.net")];
        assert_eq!(
            texts(&store.latest(&chats, 2)?),
            [Some("middle"), Some("new")]
        );
        Ok(())
    }

    #[test]
    fn keeps_the_first_copy_and_applies_edits_and_deletions() -> rusqlite::Result<()> {
        let mut store = MessageStore::in_memory()?;
        let chat = ChatId::new("1@lid");
        store.apply(&[
            MessageChange::Received(message("1@lid", "a", 10, "hello")),
            MessageChange::Received(message("1@lid", "a", 10, "duplicate")),
            MessageChange::Edited {
                chat: chat.clone(),
                id: "a".into(),
                text: Some("hi".into()),
            },
            MessageChange::Received(message("1@lid", "b", 20, "oops")),
            MessageChange::Deleted {
                chat: chat.clone(),
                id: "b".into(),
            },
        ])?;

        let messages = store.latest(&[chat], 10)?;
        assert_eq!(texts(&messages), [Some("hi"), None]);
        assert!(messages[0].edited && messages[1].deleted);
        Ok(())
    }

    #[test]
    fn receipts_only_move_a_sent_message_forward() -> rusqlite::Result<()> {
        let mut store = MessageStore::in_memory()?;
        let chat = ChatId::new("1@lid");
        let mut sent = message("1@lid", "a", 10, "hi");
        sent.from_me = true;
        store.apply(&[MessageChange::Received(sent)])?;

        let progressed = |delivery| MessageChange::Progressed {
            ids: vec!["a".into()],
            delivery,
        };
        store.apply(&[progressed(Delivery::Read), progressed(Delivery::Delivered)])?;

        let delivery = store.latest(std::slice::from_ref(&chat), 1)?[0].delivery;
        assert_eq!(delivery, Delivery::Read);
        Ok(())
    }

    #[test]
    fn keeps_one_reaction_per_person_and_drops_retracted_ones() -> rusqlite::Result<()> {
        let mut store = MessageStore::in_memory()?;
        let chat = ChatId::new("1@g.us");
        let react = |reactor: Option<&str>, emoji: Option<&str>| MessageChange::Reacted {
            chat: chat.clone(),
            message_id: "a".into(),
            reactor: reactor.map(ChatId::new),
            emoji: emoji.map(str::to_owned),
        };
        store.apply(&[
            MessageChange::Received(message("1@g.us", "a", 10, "hi")),
            react(Some("2@lid"), Some("👍")),
            react(Some("2@lid"), Some("❤️")),
            react(None, Some("😂")),
            react(Some("3@lid"), Some("😮")),
            react(Some("3@lid"), None),
        ])?;

        let messages = store.latest(std::slice::from_ref(&chat), 10)?;
        let mut reactions = store.reactions(&messages)?.remove("a").unwrap_or_default();
        reactions.sort_by(|a, b| a.emoji.cmp(&b.emoji));
        let summary: Vec<(&str, bool)> = reactions
            .iter()
            .map(|r| (r.emoji.as_str(), r.mine))
            .collect();
        assert_eq!(summary, [("❤️", false), ("😂", true)]);
        Ok(())
    }

    #[test]
    fn a_sent_message_takes_its_real_id_once_accepted() -> rusqlite::Result<()> {
        let mut store = MessageStore::in_memory()?;
        let chat = ChatId::new("1@lid");
        let mut outgoing = message("1@lid", "local-1", 10, "hey");
        outgoing.from_me = true;
        outgoing.delivery = Delivery::Sending;
        store.apply(&[MessageChange::Sending(outgoing)])?;
        store.apply(&[MessageChange::Accepted {
            chat: chat.clone(),
            local_id: "local-1".into(),
            id: "3EB0".into(),
        }])?;

        let messages = store.latest(std::slice::from_ref(&chat), 10)?;
        assert_eq!(
            (messages[0].id.as_str(), messages[0].delivery),
            ("3EB0", Delivery::Sent)
        );
        assert_eq!(store.latest_incoming(&[chat], 5)?, []);
        Ok(())
    }
}
