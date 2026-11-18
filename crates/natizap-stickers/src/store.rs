//! The favorite stickers, kept on disk so they show before the connection
//! is up, and offline.

use std::path::Path;

use natizap_protocol::{Media, Sticker};
use rusqlite::{Connection, Row, params};

/// Schema history: `MIGRATIONS[n]` upgrades the store from version `n` to
/// `n + 1`; stores are migrated, never rebuilt.
const MIGRATIONS: &[&str] = &["CREATE TABLE favorites (
        hash TEXT PRIMARY KEY,
        position INTEGER NOT NULL,
        mime TEXT,
        width INTEGER,
        height INTEGER,
        size INTEGER,
        direct_path TEXT NOT NULL,
        media_key BLOB NOT NULL,
        file_sha256 BLOB NOT NULL,
        file_enc_sha256 BLOB NOT NULL,
        lottie INTEGER NOT NULL
    );"];

/// The favorites on disk.
#[derive(Debug)]
pub(crate) struct StickerStore {
    connection: Connection,
}

impl StickerStore {
    /// Opens the store at `path`, creating it if needed.
    pub(crate) fn open(path: &Path) -> rusqlite::Result<Self> {
        Self::init(Connection::open(path)?)
    }

    #[cfg(test)]
    fn in_memory() -> rusqlite::Result<Self> {
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
        Ok(Self { connection })
    }

    /// The favorites, in order.
    pub(crate) fn load(&self) -> rusqlite::Result<Vec<Sticker>> {
        self.connection
            .prepare("SELECT * FROM favorites ORDER BY position")?
            .query_map([], sticker_from_row)?
            .collect()
    }

    /// Replaces the favorites with `stickers`, in their order.
    pub(crate) fn save(&mut self, stickers: &[Sticker]) -> rusqlite::Result<()> {
        let tx = self.connection.transaction()?;
        tx.execute("DELETE FROM favorites", [])?;
        for (position, sticker) in stickers.iter().enumerate() {
            let media = &sticker.media;
            tx.execute(
                "INSERT OR REPLACE INTO favorites (hash, position, mime, width, height, size,
                     direct_path, media_key, file_sha256, file_enc_sha256, lottie)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    sticker.hash,
                    i64::try_from(position).unwrap_or(i64::MAX),
                    media.mime,
                    media.width,
                    media.height,
                    media.size.and_then(|size| i64::try_from(size).ok()),
                    media.direct_path,
                    media.media_key,
                    media.file_sha256,
                    media.file_enc_sha256,
                    sticker.lottie,
                ],
            )?;
        }
        tx.commit()
    }
}

fn sticker_from_row(row: &Row<'_>) -> rusqlite::Result<Sticker> {
    Ok(Sticker {
        hash: row.get("hash")?,
        lottie: row.get("lottie")?,
        media: Media {
            mime: row.get("mime")?,
            width: row.get("width")?,
            height: row.get("height")?,
            size: row
                .get::<_, Option<i64>>("size")?
                .and_then(|size| u64::try_from(size).ok()),
            direct_path: row.get("direct_path")?,
            media_key: row.get("media_key")?,
            file_sha256: row.get("file_sha256")?,
            file_enc_sha256: row.get("file_enc_sha256")?,
            ..Media::default()
        },
    })
}

#[cfg(test)]
mod tests {
    use natizap_protocol::{Media, Sticker};

    use super::StickerStore;

    fn sticker(byte: u8) -> Sticker {
        Sticker::of(Media {
            mime: Some("image/webp".into()),
            direct_path: format!("/v/{byte}"),
            media_key: vec![byte; 32],
            file_sha256: vec![byte; 32],
            file_enc_sha256: vec![byte; 32],
            ..Media::default()
        })
    }

    #[test]
    fn keeps_the_favorites_in_their_order() -> rusqlite::Result<()> {
        let mut store = StickerStore::in_memory()?;
        store.save(&[sticker(2), sticker(1)])?;
        assert_eq!(store.load()?, [sticker(2), sticker(1)]);
        Ok(())
    }

    #[test]
    fn replaces_what_it_kept() -> rusqlite::Result<()> {
        let mut store = StickerStore::in_memory()?;
        store.save(&[sticker(1), sticker(2)])?;
        store.save(&[sticker(3)])?;
        assert_eq!(store.load()?, [sticker(3)]);
        Ok(())
    }
}
