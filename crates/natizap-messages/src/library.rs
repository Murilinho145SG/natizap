//! Everything a chat shared, sorted the way its media, docs and links
//! screen shows it: photos and videos, documents and audio files, and the
//! web links written in its messages, newest first.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{Image, ImageFormat, ImageSource, SharedString};
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

/// What a link's page looks like, as the sender's phone previewed it.
#[derive(Debug, Clone)]
pub(crate) struct PagePreview {
    pub(crate) title: Option<SharedString>,
    pub(crate) description: Option<SharedString>,
    pub(crate) picture: Option<Arc<Image>>,
}

/// A web link someone wrote.
#[derive(Debug, Clone)]
pub(crate) struct SharedLink {
    /// The site it leads to: `github.com`.
    pub(crate) site: SharedString,
    /// The page's preview, when the sender's phone made one.
    pub(crate) preview: Option<PagePreview>,
    /// The address to open.
    pub(crate) target: SharedString,
    /// The link as written.
    pub(crate) shown: SharedString,
    /// The message it was written in.
    pub(crate) message: SharedString,
    /// That message's id, to go to it.
    pub(crate) message_id: SharedString,
    /// When it was sent, in Unix seconds.
    pub(crate) sent_at: i64,
}

impl Shelved {
    /// What a photo or video shows as a tile: its small copy (`made`, when
    /// made since the library was read), else the preview that came with
    /// the message. Never the original, which would be decoded at full
    /// size for a tile.
    pub(crate) fn tile_picture(&self, made: Option<&PathBuf>) -> Option<ImageSource> {
        self.small
            .as_ref()
            .or(made)
            .cloned()
            .map(Into::into)
            .or_else(|| self.media.preview.clone().map(Into::into))
    }
}

impl Library {
    /// The small copies the photos and videos lack, newest first.
    pub(crate) fn small_copies_wanted(&self) -> Vec<thumbs::Wanted> {
        self.media
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                Some((index, item.original.clone()?, item.small_path.clone()))
            })
            .collect()
    }

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
            let links = web_links(text);
            // The phone previews one link per message: the one it names,
            // or the only one there is.
            let previewed = message.link.as_ref().and_then(|link| {
                let named = link.url.as_deref().map(|url| LinkKind::Web.target(url));
                links
                    .iter()
                    .position(|(target, _)| Some(target) == named.as_ref())
                    .or_else(|| (links.len() == 1).then_some(0))
                    .map(|index| (index, link))
            });
            for (index, (target, shown)) in links.into_iter().enumerate() {
                let preview =
                    previewed
                        .filter(|(previewed, _)| *previewed == index)
                        .map(|(_, link)| PagePreview {
                            title: link.title.clone().map(Into::into),
                            description: link.description.clone().map(Into::into),
                            picture: link
                                .thumbnail
                                .clone()
                                .map(|jpeg| Arc::new(Image::from_bytes(ImageFormat::Jpeg, jpeg))),
                        });
                library.links.push(SharedLink {
                    site: site_of(&target).into(),
                    preview,
                    target: target.into(),
                    shown: shown.into(),
                    message: text.into(),
                    message_id: message.id.clone().into(),
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

/// The site `target` leads to, as people name it: `github.com` for
/// `https://www.github.com/natizap`.
fn site_of(target: &str) -> String {
    let rest = target.split_once("://").map_or(target, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = host.split(':').next().unwrap_or(host);
    host.strip_prefix("www.").unwrap_or(host).to_lowercase()
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
    use super::{site_of, web_links};

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
    fn names_the_site_a_link_leads_to() {
        assert_eq!(
            site_of("https://www.github.com/natizap?tab=1"),
            "github.com"
        );
        assert_eq!(site_of("https://youtu.be/jyTXKi-Avxk"), "youtu.be");
        assert_eq!(site_of("http://Example.COM:8080/a"), "example.com");
        assert_eq!(site_of("https://user@host.dev/path"), "host.dev");
    }

    #[test]
    fn lists_a_link_written_twice_once() {
        assert_eq!(web_links("github.com, really, github.com").len(), 1);
    }
}
