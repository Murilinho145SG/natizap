//! Downloading attachments to disk.

use std::path::{Path, PathBuf};

use natizap_protocol::{ContentKind, Media, MediaOrigin};

use crate::message::Message;

/// An attachment to fetch.
#[derive(Debug, Clone)]
pub(crate) struct Fetch {
    pub(crate) message_id: String,
    pub(crate) kind: ContentKind,
    pub(crate) media: Media,
    /// The message it came in, to ask for it again once its link expired.
    pub(crate) origin: MediaOrigin,
}

impl Fetch {
    /// The attachment of `message`, if it has one that can be fetched.
    pub(crate) fn of(message: &Message) -> Option<Self> {
        Some(Self {
            message_id: message.id.clone(),
            kind: message.kind,
            media: message.media.clone()?,
            origin: MediaOrigin {
                chat: message.chat.clone(),
                message_id: message.id.clone(),
                from_me: message.from_me,
                sender: message.sender.clone(),
            },
        })
    }

    /// Where a still of the video goes inside `dir`.
    pub(crate) fn poster_path_in(&self, dir: &Path) -> PathBuf {
        let id: String = self
            .message_id
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        dir.join(format!("{id}-poster.jpg"))
    }

    /// Where the file goes inside `dir`. Documents keep their name so they
    /// open recognizably; the message id keeps names unique.
    pub(crate) fn path_in(&self, dir: &Path) -> PathBuf {
        let id: String = self
            .message_id
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        let name = match &self.media.file_name {
            Some(name) => format!("{id}-{}", sanitize(name)),
            None => format!("{id}.{}", extension(self.media.mime.as_deref(), self.kind)),
        };
        dir.join(name)
    }
}

/// Videos this small are fetched on sight when they have no preview of
/// their own, as the phone fetches media on Wi-Fi: a still is then taken
/// from the video, else it would show as an empty frame.
const SMALL_VIDEO: u64 = 16 * 1024 * 1024;

/// Photos and stickers are fetched as soon as they are on screen, as the
/// phone does, and so are small videos that came without any preview;
/// everything else waits for a click. Files whose link expired are not
/// tried: they would fail, and each failure makes the client ask WhatsApp
/// for a new media connection, which it rate-limits for the account.
pub(crate) fn fetch_on_sight(messages: &[Message]) -> Vec<Fetch> {
    messages
        .iter()
        .filter(|message| !message.media.as_ref().is_some_and(Media::link_expired))
        .filter(|message| match message.kind {
            ContentKind::Photo | ContentKind::Sticker => true,
            ContentKind::Video => is_small_and_unseen(message),
            _ => false,
        })
        .filter(|message| on_disk(message.media_file.as_deref()).is_none())
        .filter_map(Fetch::of)
        .collect()
}

/// A small video with nothing to show for it: no still taken, no preview
/// in the message and no sharper preview to download apart.
fn is_small_and_unseen(message: &Message) -> bool {
    let Some(media) = &message.media else {
        return false;
    };
    on_disk(message.media_poster.as_deref()).is_none()
        && media.thumbnail.is_none()
        && media.hd_thumbnail.is_none()
        && media.size.is_some_and(|size| size <= SMALL_VIDEO)
}

/// A video on screen that has no still yet, and where to get one.
#[derive(Debug, Clone)]
pub(crate) struct PosterJob {
    pub(crate) fetch: Fetch,
    pub(crate) source: PosterSource,
}

/// Where a video's still comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PosterSource {
    /// Taken from the downloaded video.
    Video(PathBuf),
    /// The sharper preview WhatsApp keeps apart from the video, small enough
    /// to download on sight.
    HdThumbnail(Box<Media>),
}

/// Videos on screen whose still is missing: taken from the video once it is
/// on disk, else downloaded when WhatsApp keeps a sharper preview.
pub(crate) fn posters_on_sight(messages: &[Message]) -> Vec<PosterJob> {
    messages
        .iter()
        .filter(|message| message.kind == ContentKind::Video)
        .filter(|message| on_disk(message.media_poster.as_deref()).is_none())
        .filter_map(|message| {
            let fetch = Fetch::of(message)?;
            let source = match on_disk(message.media_file.as_deref()) {
                Some(video) => PosterSource::Video(video.to_owned()),
                None => PosterSource::HdThumbnail(Box::new(
                    fetch
                        .media
                        .hd_thumbnail_download()
                        .filter(|preview| !preview.link_expired())?,
                )),
            };
            Some(PosterJob { fetch, source })
        })
        .collect()
}

fn on_disk(file: Option<&Path>) -> Option<&Path> {
    file.filter(|file| file.exists())
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn extension(mime: Option<&str>, kind: ContentKind) -> &'static str {
    let base = mime
        .and_then(|mime| mime.split(';').next())
        .unwrap_or_default()
        .trim();
    match (base, kind) {
        ("image/jpeg", _) => "jpg",
        ("image/png", _) => "png",
        ("image/webp", _) | (_, ContentKind::Sticker) => "webp",
        ("video/mp4", _) | (_, ContentKind::Video) => "mp4",
        ("audio/ogg", _) | (_, ContentKind::VoiceMessage) => "ogg",
        ("audio/mpeg", _) => "mp3",
        ("audio/mp4", _) | (_, ContentKind::Audio) => "m4a",
        ("application/pdf", _) => "pdf",
        (_, ContentKind::Photo) => "jpg",
        _ => "bin",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fetch(kind: ContentKind, mime: Option<&str>, name: Option<&str>) -> Fetch {
        Fetch {
            message_id: "3EB0:ab/c".into(),
            kind,
            media: Media {
                mime: mime.map(str::to_owned),
                file_name: name.map(str::to_owned),
                ..Media::default()
            },
            origin: MediaOrigin {
                chat: natizap_protocol::ChatId::new("1@lid"),
                message_id: "3EB0:ab/c".into(),
                from_me: false,
                sender: None,
            },
        }
    }

    fn video(id: &str, file: Option<PathBuf>, poster: Option<PathBuf>) -> Message {
        Message {
            chat: natizap_protocol::ChatId::new("1@lid"),
            id: id.into(),
            from_me: false,
            sender: None,
            author: None,
            sent_at: 0,
            kind: ContentKind::Video,
            text: None,
            edited: false,
            deleted: false,
            delivery: crate::message::Delivery::Sent,
            quote: None,
            media: Some(Media::default()),
            media_file: file,
            media_poster: poster,
            starred: false,
            sender_phone: None,
            extra: None,
            forwarding: None,
            link: None,
        }
    }

    fn hd_video(id: &str) -> Message {
        let mut message = video(id, None, None);
        message.media = Some(Media {
            hd_thumbnail: Some(natizap_protocol::HdThumbnail::default()),
            ..Media::default()
        });
        message
    }

    #[test]
    fn videos_without_a_still_get_one_from_the_file_or_a_sharper_preview() -> std::io::Result<()> {
        let dir = std::env::temp_dir().join("natizap-posters-on-sight");
        std::fs::create_dir_all(&dir)?;
        let (clip, still) = (dir.join("clip.mp4"), dir.join("clip-poster.jpg"));
        std::fs::write(&clip, b"")?;
        std::fs::write(&still, b"")?;
        let messages = [
            video("needs", Some(clip.clone()), None),
            video("has-one", Some(clip.clone()), Some(still)),
            video("stale", Some(clip.clone()), Some(dir.join("gone.jpg"))),
            video("not-downloaded", None, None),
            hd_video("sharp-preview"),
        ];
        let ids: Vec<String> = posters_on_sight(&messages)
            .into_iter()
            .map(|job| job.fetch.message_id)
            .collect();
        assert_eq!(ids, ["needs", "stale", "sharp-preview"]);
        Ok(())
    }

    fn sized_video(id: &str, size: u64, thumbnail: bool) -> Message {
        let mut message = video(id, None, None);
        message.media = Some(Media {
            size: Some(size),
            thumbnail: thumbnail.then(|| vec![0xff, 0xd8]),
            ..Media::default()
        });
        message
    }

    fn expired_photo(id: &str) -> Message {
        let mut message = video(id, None, None);
        message.kind = ContentKind::Photo;
        message.media = Some(Media {
            direct_path: "/v/t62.7118-24/1.enc?ccb=11-4&oe=5F000000".into(),
            ..Media::default()
        });
        message
    }

    #[test]
    fn fetches_small_videos_that_have_nothing_to_show() {
        let messages = [
            sized_video("small-bare", 3_000_000, false),
            sized_video("small-with-preview", 3_000_000, true),
            sized_video("large-bare", 80_000_000, false),
            hd_video("sharp-preview"),
            expired_photo("expired"),
        ];
        let ids: Vec<String> = fetch_on_sight(&messages)
            .into_iter()
            .map(|fetch| fetch.message_id)
            .collect();
        assert_eq!(ids, ["small-bare"]);
    }

    #[test]
    fn names_files_by_message_id_and_type() {
        let dir = Path::new("/media");
        assert_eq!(
            fetch(ContentKind::Photo, Some("image/jpeg"), None).path_in(dir),
            dir.join("3EB0abc.jpg")
        );
        assert_eq!(
            fetch(
                ContentKind::VoiceMessage,
                Some("audio/ogg; codecs=opus"),
                None
            )
            .path_in(dir),
            dir.join("3EB0abc.ogg")
        );
        assert_eq!(
            fetch(
                ContentKind::Document,
                Some("application/pdf"),
                Some("Nota/fiscal.pdf")
            )
            .path_in(dir),
            dir.join("3EB0abc-Nota_fiscal.pdf")
        );
    }
}
