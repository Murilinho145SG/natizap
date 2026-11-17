//! What a chat holds, as its profile sums it up: media and starred
//! messages.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{Image, ImageSource, SharedString};
use natizap_protocol::{ChatId, ContentKind};

use crate::message::Message;
use crate::preview::preview;
use crate::store::MessageStore;

/// How many recent photos and videos a profile shows.
const RECENT_MEDIA: usize = 4;

/// A chat's media and starred messages, for its profile.
#[derive(Debug, Clone, Default)]
pub struct ChatOverview {
    /// Photos, videos and documents in the chat.
    pub media: usize,
    /// The newest photos and videos, newest first.
    pub recent: Vec<MediaTile>,
    /// Messages this account starred in the chat.
    pub starred: usize,
}

/// One recent photo or video as a tile.
#[derive(Debug, Clone)]
pub struct MediaTile {
    /// The picture to show.
    pub image: Option<TileImage>,
    /// A video's length.
    pub length: Option<SharedString>,
}

/// Reads `chats`' overview from the store at `path`.
///
/// Blocks on the database; run it off the UI thread.
pub(crate) fn read(path: &Path, chats: &[ChatId]) -> ChatOverview {
    let read = || -> rusqlite::Result<ChatOverview> {
        let store = MessageStore::open(path)?;
        let (media, recent) = store.media(chats, RECENT_MEDIA)?;
        Ok(ChatOverview {
            media,
            recent: recent.iter().map(tile).collect(),
            starred: store.starred_count(chats)?,
        })
    };
    read().unwrap_or_else(|error| {
        tracing::warn!(%error, "could not read the chat's overview");
        ChatOverview::default()
    })
}

/// Where a tile's picture comes from. Plain data, unlike GPUI's image
/// sources, so it can be read off the UI thread.
#[derive(Debug, Clone)]
pub enum TileImage {
    /// The downloaded photo or a video's still.
    File(PathBuf),
    /// The preview that came with the message.
    Preview(Arc<Image>),
}

impl From<TileImage> for ImageSource {
    fn from(image: TileImage) -> Self {
        match image {
            TileImage::File(path) => path.into(),
            TileImage::Preview(preview) => preview.into(),
        }
    }
}

fn tile(message: &Message) -> MediaTile {
    let on_disk = |file: &Option<PathBuf>| file.clone().filter(|file| file.exists());
    let image = match message.kind {
        ContentKind::Photo => on_disk(&message.media_file).map(TileImage::File),
        _ => on_disk(&message.media_poster).map(TileImage::File),
    }
    .or_else(|| {
        let bytes = message.media.as_ref()?.thumbnail.clone()?;
        Some(TileImage::Preview(preview(message.kind, bytes)))
    });
    let length = message
        .media
        .as_ref()
        .and_then(|media| media.seconds)
        .filter(|_| message.kind == ContentKind::Video)
        .map(|seconds| format!("{}:{:02}", seconds / 60, seconds % 60).into());
    MediaTile { image, length }
}
