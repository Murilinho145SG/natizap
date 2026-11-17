//! Pictures on disk: one JPEG per chat, named after the chat and the moment
//! it was saved. GPUI caches images by path, so a changed picture needs a
//! new name to show.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use natizap_protocol::ChatId;

/// The pictures in `dir`, newest per chat. Unreadable entries are skipped:
/// a missing picture is fetched again.
pub(crate) fn scan(dir: &Path) -> Vec<(ChatId, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut newest: std::collections::HashMap<String, (u64, PathBuf)> =
        std::collections::HashMap::new();
    for path in entries.flatten().map(|entry| entry.path()) {
        let Some((chat, saved)) = parse_name(&path) else {
            continue;
        };
        let current = newest.entry(chat).or_insert((saved, path.clone()));
        if saved > current.0 {
            *current = (saved, path);
        }
    }
    newest
        .into_iter()
        .map(|(chat, (_, path))| (ChatId::new(&chat), path))
        .collect()
}

/// Writes `chat`'s picture to `dir` under a new name and removes older ones.
pub(crate) fn save(dir: &Path, chat: &ChatId, bytes: &[u8]) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let saved = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    let path = dir.join(format!("{}~{saved}.jpg", chat.as_str()));
    std::fs::write(&path, bytes)?;
    for entry in std::fs::read_dir(dir)?.flatten() {
        let old = entry.path();
        let same_chat = parse_name(&old).is_some_and(|(owner, _)| owner == chat.as_str());
        if same_chat && old != path {
            // A leftover only wastes a little disk; the newest still wins.
            let _ = std::fs::remove_file(old);
        }
    }
    Ok(path)
}

fn parse_name(path: &Path) -> Option<(String, u64)> {
    let name = path.file_stem()?.to_str()?;
    let (chat, saved) = name.rsplit_once('~')?;
    Some((chat.to_owned(), saved.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_the_newest_picture_of_a_chat() -> std::io::Result<()> {
        let dir = std::env::temp_dir().join("natizap-profile-files");
        let _ = std::fs::remove_dir_all(&dir);
        let chat = ChatId::new("5511999999999@s.whatsapp.net");
        save(&dir, &chat, b"old")?;
        std::thread::sleep(std::time::Duration::from_millis(5));
        let newest = save(&dir, &chat, b"new")?;
        assert_eq!(scan(&dir), [(chat, newest.clone())]);
        assert_eq!(std::fs::read(newest)?, b"new");
        Ok(())
    }
}
