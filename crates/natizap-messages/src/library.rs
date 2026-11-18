//! Everything a chat shared, sorted the way its media, docs and links
//! screen shows it: photos and videos, documents and audio files, and the
//! web links written in its messages, newest first.

use std::path::{Path, PathBuf};

use gpui::SharedString;
use natizap_protocol::{ChatId, ContentKind, LinkKind, Mark, Marked};

use crate::items::{MediaView, media_view};
use crate::message::Message;
use crate::store::MessageStore;
use crate::thumbs;

/// A chat's media, docs and links.
#[derive(Debug, Clone, Default)]
pub(crate) struct Library {
    /// Photos and videos, newest first.
    pub(crate) media: Vec<Shelved>,
    /// Documents and audio files, newest first.
    pub(crate) docs: Vec<Shelved>,
    /// Web links in messages, newest first.
    pub(crate) links: Vec<SharedLink>,
}

/// A photo, video, document or audio file.
#[derive(Debug, Clone)]
pub(crate) struct Shelved {
    pub(crate) media: MediaView,
    /// Its small copy, once made.
    pub(crate) small: Option<PathBuf>,
    /// The downloaded photo or video still a small copy is made from, while
    /// there is none.
    pub(crate) original: Option<PathBuf>,
    /// Where its small copy goes.
    pub(crate) small_path: PathBuf,
    /// When it was sent, in Unix seconds.
    pub(crate) sent_at: i64,
    /// Its caption, or a document's file name, to search by.
    pub(crate) words: String,
}

/// A web link someone wrote.
#[derive(Debug, Clone)]
pub(crate) struct SharedLink {
    /// The address to open.
    pub(crate) target: SharedString,
    /// The link as written.
    pub(crate) shown: SharedString,
    /// The message it was written in.
    pub(crate) message: SharedString,
    /// When it was sent, in Unix seconds.
    pub(crate) sent_at: i64,
}

impl Library {
    /// Reads `chats`' library from the store at `path`, with the small
    /// copies already made in the media folder `media_dir`.
    ///
    /// Blocks on the database; run it off the UI thread.
    pub(crate) fn read(path: &Path, media_dir: &Path, chats: &[ChatId]) -> Self {
        let messages = MessageStore::open(path).and_then(|store| store.library(chats));
        match messages {
            Ok(messages) => Self::of(&messages, media_dir),
            Err(error) => {
                tracing::warn!(%error, "could not read the chat's media, docs and links");
                Self::default()
            }
        }
    }

    /// Sorts `messages`, newest first, onto the library's shelves.
    fn of(messages: &[Message], media_dir: &Path) -> Self {
        let mut library = Self::default();
        for message in messages {
            let text = message.text.as_deref().unwrap_or_default();
            for (target, shown) in web_links(text) {
                library.links.push(SharedLink {
                    target: target.into(),
                    shown: shown.into(),
                    message: text.into(),
                    sent_at: message.sent_at,
                });
            }
            let shelf = match message.kind {
                ContentKind::Photo | ContentKind::Video => &mut library.media,
                ContentKind::Document | ContentKind::Audio => &mut library.docs,
                _ => continue,
            };
            let Some(media) = media_view(message) else {
                continue;
            };
            let words = [media.name.as_deref(), message.text.as_deref()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
            let small_path = thumbs::path_in(media_dir, &message.id);
            let small = Some(small_path.clone()).filter(|small| small.exists());
            let on_disk = |file: &Option<PathBuf>| file.clone().filter(|file| file.exists());
            let original = match message.kind {
                _ if small.is_some() => None,
                ContentKind::Photo => on_disk(&message.media_file),
                ContentKind::Video => on_disk(&message.media_poster),
                _ => None,
            };
            shelf.push(Shelved {
                small,
                original,
                small_path,
                media,
                sent_at: message.sent_at,
                words,
            });
        }
        library
    }
}

/// The web links in message `text`, each once: where it leads and how it
/// was written.
fn web_links(text: &str) -> Vec<(String, String)> {
    let marked = Marked::parse(text);
    let mut links: Vec<(String, String)> = Vec::new();
    for (range, mark) in &marked.marks {
        let Mark::Link(LinkKind::Web) = mark else {
            continue;
        };
        let Some(shown) = marked.text.get(range.clone()) else {
            continue;
        };
        let target = LinkKind::Web.target(shown);
        if !links.iter().any(|(seen, _)| *seen == target) {
            links.push((target, shown.to_owned()));
        }
    }
    links
}

#[cfg(test)]
mod tests {
    use super::web_links;

    #[test]
    fn finds_web_links_and_where_they_lead() {
        assert_eq!(
            web_links("look: github.com/natizap and https://gpui.rs/docs"),
            vec![
                (
                    "https://github.com/natizap".to_owned(),
                    "github.com/natizap".to_owned()
                ),
                (
                    "https://gpui.rs/docs".to_owned(),
                    "https://gpui.rs/docs".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn leaves_out_emails_and_phone_numbers() {
        assert_eq!(
            web_links("write to ana@example.com or call +55 11 98765-0001"),
            Vec::<(String, String)>::new()
        );
    }

    #[test]
    fn lists_a_link_written_twice_once() {
        assert_eq!(web_links("github.com, really, github.com").len(), 1);
    }
}
