//! The favorite stickers: read from WhatsApp each time the connection comes
//! up, kept on disk in between, with their files downloaded for showing.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

use gpui::prelude::*;
use gpui::{Context, Entity, Task};
use natizap_protocol::whatsapp_rust::prelude::EventKind;
use natizap_protocol::{ContentKind, Protocol, Sticker};

use crate::store::StickerStore;

/// Favorite stickers downloaded at the same time.
const DOWNLOADS_AT_ONCE: usize = 3;

/// A favorite sticker, and its file once downloaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Favorite {
    /// The sticker.
    pub sticker: Sticker,
    /// Where its file is, once here; Lottie stickers have none to show.
    pub file: Option<PathBuf>,
}

/// The account's favorite stickers, as its phone keeps them. Notifies
/// observers when they change.
pub struct StickerShelf {
    protocol: Entity<Protocol>,
    store_path: PathBuf,
    /// Where sticker files are kept.
    dir: PathBuf,
    favorites: Vec<Favorite>,
    /// Stickers whose file is being fetched or waits to be, by hash.
    fetching: HashSet<String>,
    /// Stickers waiting for their file to be downloaded, next first.
    queue: VecDeque<Sticker>,
    /// Downloads under way.
    downloading: usize,
    _tasks: [Task<()>; 2],
}

impl std::fmt::Debug for StickerShelf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StickerShelf")
            .field("favorites", &self.favorites.len())
            .finish_non_exhaustive()
    }
}

impl StickerShelf {
    /// The favorites kept at `store_path`, with their files in `dir`,
    /// brought up to date from WhatsApp whenever the connection comes up.
    pub fn new(
        protocol: &Entity<Protocol>,
        store_path: PathBuf,
        dir: PathBuf,
        cx: &mut Context<Self>,
    ) -> Self {
        let reading = store_path.clone();
        let loading = cx.spawn(async move |this, cx| {
            let kept = cx
                .background_spawn(async move { StickerStore::open(&reading)?.load() })
                .await;
            match kept {
                Ok(kept) => {
                    let _ = this.update(cx, |shelf, cx| shelf.replace(kept, false, cx));
                }
                Err(error) => tracing::warn!(%error, "could not read the favorite stickers"),
            }
        });
        let connects = protocol.read(cx).subscribe(&[EventKind::Connected]);
        let refreshing = cx.spawn(async move |this, cx| {
            while connects.recv().await.is_ok() {
                if this.update(cx, Self::refresh).is_err() {
                    break;
                }
            }
        });
        Self {
            protocol: protocol.clone(),
            store_path,
            dir,
            favorites: Vec::new(),
            fetching: HashSet::new(),
            queue: VecDeque::new(),
            downloading: 0,
            _tasks: [loading, refreshing],
        }
    }

    /// The favorites, newest first, as the phone shows them.
    pub fn favorites(&self) -> impl Iterator<Item = &Favorite> {
        self.favorites.iter().rev()
    }

    /// The hashes of the favorites, to tell whether a sticker is one.
    #[must_use]
    pub fn hashes(&self) -> HashSet<String> {
        self.favorites
            .iter()
            .map(|favorite| favorite.sticker.hash.clone())
            .collect()
    }

    /// Keeps `sticker` among the favorites, or takes it out, here and on
    /// the account's other devices. `file` is where it already is on disk,
    /// when a message brought it.
    pub fn set_favorite(
        &mut self,
        sticker: Sticker,
        file: Option<PathBuf>,
        favorite: bool,
        cx: &mut Context<Self>,
    ) {
        let mut stickers: Vec<Sticker> = self
            .favorites
            .iter()
            .map(|kept| kept.sticker.clone())
            .filter(|kept| kept.hash != sticker.hash)
            .collect();
        if favorite {
            stickers.push(sticker.clone());
        }
        // A file a message brought is copied rather than downloaded again,
        // off the UI thread.
        let copying = file
            .filter(|_| favorite)
            .map(|file| (file, self.file_path(&sticker.hash)))
            .filter(|_| self.fetching.insert(sticker.hash.clone()));
        self.replace(stickers, true, cx);
        if let Some((from, to)) = copying {
            let hash = sticker.hash.clone();
            cx.spawn(async move |this, cx| {
                let kept = to.clone();
                let copied = cx
                    .background_spawn(async move {
                        let bytes = std::fs::read(&from).map_err(|error| error.to_string())?;
                        save(&kept, &bytes)
                    })
                    .await;
                let _ = this.update(cx, |shelf, cx| {
                    shelf.fetching.remove(&hash);
                    match copied {
                        Ok(()) => shelf.found(&hash, to, cx),
                        // Downloaded instead.
                        Err(_) => shelf.fetch_missing(cx),
                    }
                });
            })
            .detach();
        }
        let syncing = self
            .protocol
            .read(cx)
            .set_favorite_sticker(sticker, favorite, cx);
        cx.background_spawn(async move {
            if let Err(error) = syncing.await {
                tracing::warn!(%error, "favorite sticker not synced to the other devices");
            }
        })
        .detach();
    }

    /// Reads the favorites from WhatsApp, as the phone keeps them now.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let reading = self.protocol.read(cx).favorite_stickers(cx);
        cx.spawn(async move |this, cx| match reading.await {
            Ok(stickers) => {
                tracing::info!(count = stickers.len(), "favorite stickers read");
                let _ = this.update(cx, |shelf, cx| shelf.replace(stickers, true, cx));
            }
            Err(error) => tracing::warn!(%error, "could not read the favorite stickers"),
        })
        .detach();
    }

    /// Shows `stickers` as the favorites, keeping them on disk when
    /// `save`, and fetches the files they lack.
    fn replace(&mut self, stickers: Vec<Sticker>, save: bool, cx: &mut Context<Self>) {
        if save {
            let (path, kept) = (self.store_path.clone(), stickers.clone());
            cx.background_spawn(async move {
                if let Err(error) =
                    StickerStore::open(&path).and_then(|mut store| store.save(&kept))
                {
                    tracing::warn!(%error, "could not keep the favorite stickers");
                }
            })
            .detach();
        }
        self.favorites = stickers
            .into_iter()
            .map(|sticker| {
                let file = Some(self.file_path(&sticker.hash)).filter(|file| file.exists());
                Favorite { sticker, file }
            })
            .collect();
        self.fetch_missing(cx);
        cx.notify();
    }

    /// Queues the files of the favorites that have none yet, newest first,
    /// as the picker shows them.
    fn fetch_missing(&mut self, cx: &mut Context<Self>) {
        let missing: Vec<Sticker> = self
            .favorites
            .iter()
            .rev()
            .filter(|favorite| favorite.file.is_none() && !favorite.sticker.lottie)
            .map(|favorite| favorite.sticker.clone())
            .filter(|sticker| self.fetching.insert(sticker.hash.clone()))
            .collect();
        self.queue.extend(missing);
        self.pump(cx);
    }

    /// Starts queued downloads, a few at a time: a hundred favorites asked
    /// for at once, just as the connection comes up, mostly time out.
    fn pump(&mut self, cx: &mut Context<Self>) {
        while self.downloading < DOWNLOADS_AT_ONCE {
            let Some(sticker) = self.queue.pop_front() else {
                return;
            };
            self.downloading += 1;
            let path = self.file_path(&sticker.hash);
            let downloading =
                self.protocol
                    .read(cx)
                    .download(ContentKind::Sticker, sticker.media.clone(), cx);
            cx.spawn(async move |this, cx| {
                let saved = match downloading.await {
                    Ok(bytes) => {
                        let path = path.clone();
                        cx.background_spawn(async move { save(&path, &bytes) })
                            .await
                    }
                    Err(error) => Err(error.to_string()),
                };
                let _ = this.update(cx, |shelf, cx| {
                    shelf.downloading -= 1;
                    shelf.fetching.remove(&sticker.hash);
                    match saved {
                        Ok(()) => shelf.found(&sticker.hash, path, cx),
                        Err(error) => tracing::warn!(
                            %error,
                            sticker = sticker.hash,
                            "could not download a favorite sticker"
                        ),
                    }
                    shelf.pump(cx);
                });
            })
            .detach();
        }
    }

    /// Shows the favorite `hash` with its file, now at `file`.
    fn found(&mut self, hash: &str, file: PathBuf, cx: &mut Context<Self>) {
        if let Some(favorite) = self
            .favorites
            .iter_mut()
            .find(|favorite| favorite.sticker.hash == hash)
        {
            favorite.file = Some(file);
            cx.notify();
        }
    }

    /// Where `sticker`'s file is kept, once it is here: a message sent
    /// before it arrives shows it from there as soon as it does.
    #[must_use]
    pub fn file_for(&self, sticker: &Sticker) -> PathBuf {
        self.file_path(&sticker.hash)
    }

    /// Where the file of the sticker `hash` is kept.
    fn file_path(&self, hash: &str) -> PathBuf {
        file_path_in(&self.dir, hash)
    }
}

/// Where the file of the sticker `hash` is kept in `dir`: its hash, made
/// safe for a file name.
fn file_path_in(dir: &Path, hash: &str) -> PathBuf {
    let name: String = hash
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    dir.join(format!("{name}.webp"))
}

fn save(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    std::fs::write(path, bytes).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::file_path_in;

    #[test]
    fn names_files_safely_by_hash() {
        assert_eq!(
            file_path_in(Path::new("/stickers"), "ab+c/d=="),
            Path::new("/stickers/ab_c_d__.webp")
        );
    }
}
