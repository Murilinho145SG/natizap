//! The call history on disk in `calls.db`, so it shows at once and offline.

use std::path::Path;

use natizap_protocol::{CallOutcome, CallRecord, ChatId};
use rusqlite::{Connection, params};

/// Schema history: `MIGRATIONS[n]` upgrades a store from version `n` to
/// `n + 1`. Stores are migrated, never rebuilt.
const MIGRATIONS: &[&str] = &["CREATE TABLE calls (
        id TEXT PRIMARY KEY,
        peer TEXT NOT NULL,
        grp TEXT,
        incoming INTEGER NOT NULL,
        video INTEGER NOT NULL,
        outcome TEXT NOT NULL,
        at INTEGER NOT NULL,
        seconds INTEGER
    );
    CREATE INDEX calls_by_time ON calls (at);"];

/// How many calls the history shows.
const SHOWN: i64 = 500;

pub(crate) struct CallStore {
    connection: Connection,
}

impl CallStore {
    pub(crate) fn open(path: &Path) -> rusqlite::Result<Self> {
        Self::init(Connection::open(path)?)
    }

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

    /// Keeps `calls`; a call already kept takes the newer word on it.
    pub(crate) fn keep(&mut self, calls: &[CallRecord]) -> rusqlite::Result<()> {
        let tx = self.connection.transaction()?;
        {
            let mut insert = tx.prepare_cached(
                "INSERT OR REPLACE INTO calls VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for call in calls {
                insert.execute(params![
                    call.id,
                    call.peer.as_str(),
                    call.group.as_ref().map(ChatId::as_str),
                    call.incoming,
                    call.video,
                    call.outcome.key(),
                    call.at,
                    call.seconds,
                ])?;
            }
        }
        tx.commit()
    }

    /// Keeps `calls` that are not kept yet, leaving those already kept as
    /// they are.
    pub(crate) fn keep_new(&mut self, calls: &[CallRecord]) -> rusqlite::Result<()> {
        let tx = self.connection.transaction()?;
        {
            let mut insert = tx.prepare_cached(
                "INSERT OR IGNORE INTO calls VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for call in calls {
                insert.execute(params![
                    call.id,
                    call.peer.as_str(),
                    call.group.as_ref().map(ChatId::as_str),
                    call.incoming,
                    call.video,
                    call.outcome.key(),
                    call.at,
                    call.seconds,
                ])?;
            }
        }
        tx.commit()
    }

    /// The newest calls, newest first.
    pub(crate) fn recent(&self) -> rusqlite::Result<Vec<CallRecord>> {
        self.connection
            .prepare("SELECT * FROM calls ORDER BY at DESC LIMIT ?1")?
            .query_map(params![SHOWN], |row| {
                Ok(CallRecord {
                    id: row.get("id")?,
                    peer: ChatId::new(&row.get::<_, String>("peer")?),
                    group: row
                        .get::<_, Option<String>>("grp")?
                        .map(|group| ChatId::new(&group)),
                    incoming: row.get("incoming")?,
                    video: row.get("video")?,
                    outcome: CallOutcome::from_key(&row.get::<_, String>("outcome")?)
                        .unwrap_or(CallOutcome::Missed),
                    at: row.get("at")?,
                    seconds: row.get("seconds")?,
                })
            })?
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(id: &str, at: i64, outcome: CallOutcome) -> CallRecord {
        CallRecord {
            id: id.into(),
            peer: ChatId::new("1@lid"),
            group: None,
            incoming: true,
            video: true,
            outcome,
            at,
            seconds: Some(12),
        }
    }

    #[test]
    fn keeps_calls_newest_first_and_takes_later_word() -> rusqlite::Result<()> {
        let mut store = CallStore::in_memory()?;
        store.keep(&[
            call("a", 10, CallOutcome::Missed),
            call("b", 20, CallOutcome::Answered),
        ])?;
        // The history sync says more about call "a".
        store.keep(&[call("a", 10, CallOutcome::Answered)])?;
        let kept = store.recent()?;
        let seen: Vec<(&str, CallOutcome)> = kept
            .iter()
            .map(|call| (call.id.as_str(), call.outcome))
            .collect();
        assert_eq!(
            seen,
            [("b", CallOutcome::Answered), ("a", CallOutcome::Answered)]
        );
        assert_eq!(kept[0].seconds, Some(12));
        Ok(())
    }
}
