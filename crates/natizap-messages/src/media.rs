//! Downloading attachments to disk.

use std::path::{Path, PathBuf};

use natizap_protocol::{ContentKind, Media};

use crate::message::Message;

/// An attachment to fetch.
#[derive(Debug, Clone)]
pub(crate) struct Fetch {
    pub(crate) message_id: String,
    pub(crate) kind: ContentKind,
    pub(crate) media: Media,
}

impl Fetch {
    /// The attachment of `message`, if it has one that can be fetched.
    pub(crate) fn of(message: &Message) -> Option<Self> {
        Some(Self {
            message_id: message.id.clone(),
            kind: message.kind,
            media: message.media.clone()?,
        })
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

/// Photos and stickers are fetched as soon as they are on screen, as the
/// phone does; everything else waits for a click.
pub(crate) fn fetch_on_sight(messages: &[Message]) -> Vec<Fetch> {
    messages
        .iter()
        .filter(|message| matches!(message.kind, ContentKind::Photo | ContentKind::Sticker))
        .filter(|message| {
            message
                .media_file
                .as_deref()
                .is_none_or(|file| !file.exists())
        })
        .filter_map(Fetch::of)
        .collect()
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
        }
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
