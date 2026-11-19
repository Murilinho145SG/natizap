//! The status updates of the last day, and who saw this account's own, on
//! disk in `status.db`, so they show at once and offline.

use std::path::{Path, PathBuf};

use natizap_protocol::{ChatId, ContentKind, Media, StatusLink};
use rusqlite::{Connection, Row, params};

use crate::post::{DAY, Post};

/// Schema history: `MIGRATIONS[n]` upgrades a store from version `n` to
/// `n + 1`. Stores are migrated, never rebuilt.
const MIGRATIONS: &[&str] = &[
    "CREATE TABLE posts (
        poster TEXT NOT NULL,
        id TEXT NOT NULL,
        sent_at INTEGER NOT NULL,
        kind TEXT NOT NULL,
        text TEXT,
        background INTEGER,
        author TEXT,
        mine INTEGER NOT NULL,
        seen INTEGER NOT NULL,
        file TEXT,
        media_mime TEXT,
        media_size INTEGER,
        media_width INTEGER,
        media_height INTEGER,
        media_seconds INTEGER,
        media_thumbnail BLOB,
        media_direct_path TEXT,
        media_key BLOB,
        media_sha256 BLOB,
        media_enc_sha256 BLOB,
        PRIMARY KEY (poster, id)
    );
    CREATE TABLE viewers (
        id TEXT NOT NULL,
        viewer TEXT NOT NULL,
        seen_at INTEGER NOT NULL,
        PRIMARY KEY (id, viewer)
    );",
    "ALTER TABLE posts ADD COLUMN link_label TEXT;
    ALTER TABLE posts ADD COLUMN link_url TEXT;",
];

/// Someone who saw one of this account's posts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Viewer {
    /// Which post.
    pub id: String,
    /// Who saw it.
    pub viewer: ChatId,
    /// When, in Unix seconds.
    pub seen_at: i64,
}

/// A change to the stored status updates.
#[derive(Debug, Clone)]
pub(crate) enum Change {
    /// A post arrived; one already kept keeps its seen state and file.
    Posted(Box<Post>),
    /// The poster deleted it.
    Removed { poster: ChatId, id: String },
    /// This account saw it.
    Seen { poster: ChatId, id: String },
    /// This account saw it on another device, which names it by id alone.
    SeenElsewhere { id: String },
    /// Its photo or video was downloaded to `file`.
    Downloaded {
        poster: ChatId,
        id: String,
        file: PathBuf,
    },
    /// Someone saw one of this account's posts.
    Viewed(Viewer),
}

pub(crate) struct StatusStore {
    connection: Connection,
}

impl StatusStore {
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

    pub(crate) fn apply(&mut self, changes: &[Change]) -> rusqlite::Result<()> {
        let tx = self.connection.transaction()?;
        for change in changes {
            match change {
                Change::Posted(post) => insert(&tx, post)?,
                Change::Removed { poster, id } => {
                    tx.execute(
                        "DELETE FROM posts WHERE poster = ?1 AND id = ?2",
                        params![poster.as_str(), id],
                    )?;
                }
                Change::Seen { poster, id } => {
                    tx.execute(
                        "UPDATE posts SET seen = 1 WHERE poster = ?1 AND id = ?2",
                        params![poster.as_str(), id],
                    )?;
                }
                Change::SeenElsewhere { id } => {
                    tx.execute("UPDATE posts SET seen = 1 WHERE id = ?1", params![id])?;
                }
                Change::Downloaded { poster, id, file } => {
                    tx.execute(
                        "UPDATE posts SET file = ?3 WHERE poster = ?1 AND id = ?2",
                        params![poster.as_str(), id, file.to_string_lossy()],
                    )?;
                }
                Change::Viewed(viewer) => {
                    tx.execute(
                        "INSERT OR IGNORE INTO viewers VALUES (?1, ?2, ?3)",
                        params![viewer.id, viewer.viewer.as_str(), viewer.seen_at],
                    )?;
                }
            }
        }
        tx.commit()
    }

    /// Every post of the day before `now`, oldest first; older ones and
    /// their viewers are forgotten.
    pub(crate) fn recent(&mut self, now: i64) -> rusqlite::Result<Vec<Post>> {
        let since = now - DAY;
        self.connection.execute(
            "DELETE FROM viewers WHERE id IN (SELECT id FROM posts WHERE sent_at < ?1)",
            params![since],
        )?;
        self.connection
            .execute("DELETE FROM posts WHERE sent_at < ?1", params![since])?;
        self.connection
            .prepare("SELECT * FROM posts ORDER BY sent_at")?
            .query_map([], post_from_row)?
            .collect()
    }

    /// Who saw this account's posts, earliest first.
    pub(crate) fn viewers(&self) -> rusqlite::Result<Vec<Viewer>> {
        self.connection
            .prepare("SELECT id, viewer, seen_at FROM viewers ORDER BY seen_at")?
            .query_map([], |row| {
                Ok(Viewer {
                    id: row.get(0)?,
                    viewer: ChatId::new(&row.get::<_, String>(1)?),
                    seen_at: row.get(2)?,
                })
            })?
            .collect()
    }
}

fn insert(tx: &rusqlite::Transaction<'_>, post: &Post) -> rusqlite::Result<()> {
    let media = post.media.as_ref();
    tx.prepare_cached(
        "INSERT INTO posts (poster, id, sent_at, kind, text, background, author, mine, seen,
         file, media_mime, media_size, media_width, media_height, media_seconds,
         media_thumbnail, media_direct_path, media_key, media_sha256, media_enc_sha256,
         link_label, link_url)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
         ?18, ?19, ?20, ?21, ?22)
         ON CONFLICT (poster, id) DO UPDATE SET text = excluded.text",
    )?
    .execute(params![
        post.poster.as_str(),
        post.id,
        post.sent_at,
        post.kind.label(),
        post.text,
        post.background,
        post.author,
        post.mine,
        post.seen,
        post.file.as_ref().map(|file| file.to_string_lossy()),
        media.and_then(|m| m.mime.as_ref()),
        media
            .and_then(|m| m.size)
            .and_then(|size| i64::try_from(size).ok()),
        media.and_then(|m| m.width),
        media.and_then(|m| m.height),
        media.and_then(|m| m.seconds),
        media.and_then(|m| m.thumbnail.as_ref()),
        media.map(|m| &m.direct_path),
        media.map(|m| &m.media_key),
        media.map(|m| &m.file_sha256),
        media.map(|m| &m.file_enc_sha256),
        post.link.as_ref().map(|link| &link.label),
        post.link.as_ref().and_then(|link| link.url.as_ref()),
    ])?;
    Ok(())
}

fn post_from_row(row: &Row<'_>) -> rusqlite::Result<Post> {
    let direct_path: Option<String> = row.get("media_direct_path")?;
    let media = match direct_path {
        Some(direct_path) => Some(Media {
            mime: row.get("media_mime")?,
            size: row
                .get::<_, Option<i64>>("media_size")?
                .and_then(|size| u64::try_from(size).ok()),
            width: row.get("media_width")?,
            height: row.get("media_height")?,
            seconds: row.get("media_seconds")?,
            thumbnail: row.get("media_thumbnail")?,
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
            ..Media::default()
        }),
        None => None,
    };
    Ok(Post {
        poster: ChatId::new(&row.get::<_, String>("poster")?),
        id: row.get("id")?,
        sent_at: row.get("sent_at")?,
        kind: ContentKind::from_label(&row.get::<_, String>("kind")?).unwrap_or(ContentKind::Text),
        text: row.get("text")?,
        background: row.get("background")?,
        author: row.get("author")?,
        media,
        file: row.get::<_, Option<String>>("file")?.map(PathBuf::from),
        seen: row.get("seen")?,
        mine: row.get("mine")?,
        link: row
            .get::<_, Option<String>>("link_label")?
            .map(|label| -> rusqlite::Result<StatusLink> {
                Ok(StatusLink {
                    label,
                    url: row.get("link_url")?,
                })
            })
            .transpose()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(id: &str, sent_at: i64) -> Post {
        Post {
            poster: ChatId::new("1@lid"),
            id: id.into(),
            sent_at,
            kind: ContentKind::Photo,
            text: Some("caption".into()),
            background: None,
            author: Some("Ana".into()),
            media: Some(Media {
                direct_path: "/v/1".into(),
                media_key: vec![1, 2],
                ..Media::default()
            }),
            file: None,
            seen: false,
            mine: false,
            link: None,
        }
    }

    #[test]
    fn keeps_posts_seen_state_and_files_across_repeats() -> rusqlite::Result<()> {
        let mut store = StatusStore::in_memory()?;
        let now = 10 * DAY;
        let poster = ChatId::new("1@lid");
        store.apply(&[
            Change::Posted(Box::new(post("a", now - 10))),
            Change::Seen {
                poster: poster.clone(),
                id: "a".into(),
            },
            Change::Downloaded {
                poster: poster.clone(),
                id: "a".into(),
                file: PathBuf::from("/tmp/a.jpg"),
            },
            // Delivered again, as live and history both do.
            Change::Posted(Box::new(post("a", now - 10))),
        ])?;
        let kept = store.recent(now)?;
        assert_eq!(kept.len(), 1);
        assert!(kept[0].seen);
        assert_eq!(kept[0].file.as_deref(), Some(Path::new("/tmp/a.jpg")));
        assert_eq!(
            kept[0].media.as_ref().map(|m| m.media_key.clone()),
            Some(vec![1, 2])
        );
        Ok(())
    }

    #[test]
    fn forgets_what_is_older_than_a_day_or_deleted() -> rusqlite::Result<()> {
        let mut store = StatusStore::in_memory()?;
        let now = 10 * DAY;
        store.apply(&[
            Change::Posted(Box::new(post("old", now - DAY - 1))),
            Change::Posted(Box::new(post("gone", now - 5))),
            Change::Posted(Box::new(post("kept", now - 5))),
            Change::Removed {
                poster: ChatId::new("1@lid"),
                id: "gone".into(),
            },
            Change::Viewed(Viewer {
                id: "old".into(),
                viewer: ChatId::new("2@lid"),
                seen_at: now - DAY,
            }),
        ])?;
        let ids: Vec<String> = store.recent(now)?.into_iter().map(|p| p.id).collect();
        assert_eq!(ids, ["kept"]);
        assert_eq!(store.viewers()?, Vec::new());
        Ok(())
    }
}
