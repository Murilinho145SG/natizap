//! Everyone's status updates of the last day, kept in step with WhatsApp:
//! posts arrive and download in the background, are marked seen on every
//! device as they are watched, and this account posts its own.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;
use gpui::{AppContext as _, Context, Entity, SharedString, Task};
use natizap_protocol::whatsapp_rust::prelude::Event;
use natizap_protocol::{ActionError, ChatId, ContentKind, Protocol, Unread};

use crate::post::{Post, Poster, gather};
use crate::read::{STATUS_EVENTS, changes};
use crate::store::{Change, StatusStore, Viewer};

/// Requests to the store's worker.
enum Input {
    Event(Arc<Event>),
    Change(Change),
}

/// What the worker read back after a change.
#[derive(Default)]
struct Snapshot {
    posts: Arc<[Post]>,
    viewers: Arc<[Viewer]>,
}

/// Why a status could not be posted.
#[derive(Debug, thiserror::Error)]
pub enum PostError {
    /// The file could not be made ready to post.
    #[error(transparent)]
    File(#[from] natizap_outbox::PrepareError),
    /// Only photos and videos post as status updates.
    #[error("only photos and MP4 videos can be posted as a status")]
    NotPhotoOrVideo,
    /// WhatsApp did not take it.
    #[error(transparent)]
    Refused(#[from] ActionError),
    /// Nobody to show it to yet: the contacts are not known.
    #[error("your contacts are not known yet")]
    NoAudience,
}

/// The status updates of the people this account knows, and its own.
pub struct StatusBook {
    protocol: Entity<Protocol>,
    inputs: async_channel::Sender<Input>,
    snapshot: Snapshot,
    media_dir: PathBuf,
    /// Posts whose photo or video is downloading, by id.
    fetching: HashSet<String>,
    /// Posts whose file could not be downloaded this run, by id, so they
    /// do not keep the others waiting.
    failed: HashSet<String>,
    /// The post on screen, downloaded before the others.
    wanted: Option<String>,
    /// What people are called, by every address they go by.
    titles: HashMap<ChatId, SharedString>,
    /// Who this account's own posts go to.
    audience: Vec<ChatId>,
    _tasks: [Task<()>; 3],
}

impl std::fmt::Debug for StatusBook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatusBook")
            .field("posts", &self.snapshot.posts.len())
            .finish_non_exhaustive()
    }
}

impl StatusBook {
    /// Keeps status updates in the store at `path` and their files in
    /// `media_dir`, learning them through `protocol`.
    pub fn new(
        protocol: &Entity<Protocol>,
        path: PathBuf,
        media_dir: PathBuf,
        cx: &mut Context<Self>,
    ) -> Self {
        let (inputs_tx, inputs) = async_channel::unbounded();
        let events = protocol.read(cx).subscribe(STATUS_EVENTS);
        let forward_tx = inputs_tx.clone();
        let forward = cx.background_spawn(async move {
            while let Ok(event) = events.recv().await {
                if forward_tx.send(Input::Event(event)).await.is_err() {
                    break;
                }
            }
        });
        // Watched on the phone.
        let reads = protocol.read(cx).reads_elsewhere(cx);
        let reads_tx = inputs_tx.clone();
        cx.background_spawn(async move {
            while let Ok(read) = reads.recv().await {
                if read.chat.is_conversation() {
                    continue;
                }
                for id in read.ids {
                    let seen = Input::Change(Change::SeenElsewhere { id });
                    if reads_tx.send(seen).await.is_err() {
                        return;
                    }
                }
            }
        })
        .detach();
        let (snapshots_tx, snapshots) = async_channel::unbounded();
        let work = cx.background_spawn(work(path, inputs, snapshots_tx));
        let apply = cx.spawn(async move |this, cx| {
            while let Ok(mut snapshot) = snapshots.recv().await {
                while let Ok(newer) = snapshots.try_recv() {
                    snapshot = newer;
                }
                let applied = this.update(cx, |book, cx| {
                    book.snapshot = snapshot;
                    book.fetch_next(cx);
                    cx.notify();
                });
                if applied.is_err() {
                    break;
                }
            }
        });
        Self {
            protocol: protocol.clone(),
            inputs: inputs_tx,
            snapshot: Snapshot::default(),
            media_dir,
            fetching: HashSet::new(),
            failed: HashSet::new(),
            wanted: None,
            titles: HashMap::new(),
            audience: Vec::new(),
            _tasks: [forward, work, apply],
        }
    }

    /// This account's own posts of the last day, if any.
    #[must_use]
    pub fn mine(&self) -> Option<Poster> {
        gather(&self.snapshot.posts, Utc::now().timestamp()).0
    }

    /// Everyone else's posts of the last day, by poster, newest first.
    #[must_use]
    pub fn others(&self) -> Vec<Poster> {
        gather(&self.snapshot.posts, Utc::now().timestamp()).1
    }

    /// The people with a status, and whether it holds anything unseen.
    #[must_use]
    pub fn rings(&self) -> HashMap<ChatId, bool> {
        self.others()
            .into_iter()
            .map(|poster| {
                let unseen = poster.unseen();
                (poster.poster, unseen)
            })
            .collect()
    }

    /// Who saw this account's post `id`, earliest first.
    #[must_use]
    pub fn viewers_of(&self, id: &str) -> Vec<Viewer> {
        self.snapshot
            .viewers
            .iter()
            .filter(|viewer| viewer.id == id)
            .cloned()
            .collect()
    }

    /// What `chat` is called: the name it is saved under, else the name
    /// its posts came with, else its number.
    #[must_use]
    pub fn title(&self, chat: &ChatId) -> SharedString {
        self.titles.get(chat).cloned().unwrap_or_else(|| {
            self.snapshot
                .posts
                .iter()
                .rev()
                .find(|post| post.poster == *chat)
                .and_then(|post| post.author.clone())
                .or_else(|| chat.phone_number())
                .unwrap_or_else(|| chat.as_str().to_owned())
                .into()
        })
    }

    /// Names people by every address they go by.
    pub fn set_titles(&mut self, titles: HashMap<ChatId, SharedString>, cx: &mut Context<Self>) {
        self.titles = titles;
        cx.notify();
    }

    /// Who this account's posts go to: its contacts, as the phone's "My
    /// contacts" status privacy shows them.
    pub fn set_audience(&mut self, audience: Vec<ChatId>) {
        self.audience = audience;
    }

    /// Takes `posts` kept elsewhere before this store had them, such as the
    /// message store's; posts already here keep what is known of them.
    pub fn import(&self, posts: Vec<Post>) {
        for post in posts {
            self.send(Change::Posted(Box::new(post)));
        }
    }

    /// Marks `post` seen, here and on the poster's phone.
    pub fn mark_seen(&mut self, post: &Post, cx: &mut Context<Self>) {
        if post.seen || post.mine {
            return;
        }
        self.send(Change::Seen {
            poster: post.poster.clone(),
            id: post.id.clone(),
        });
        let seeing = self.protocol.read(cx).mark_read(
            &ChatId::new("status@broadcast"),
            vec![Unread {
                id: post.id.clone(),
                sender: Some(post.poster.clone()),
            }],
            cx,
        );
        cx.background_spawn(async move {
            if let Err(error) = seeing.await {
                tracing::warn!(%error, "could not tell the poster a status was seen");
            }
        })
        .detach();
    }

    /// Posts `text` on a `background` colour (0xAARRGGBB).
    pub fn post_text(
        &mut self,
        text: String,
        background: u32,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), PostError>> {
        if self.audience.is_empty() {
            return Task::ready(Err(PostError::NoAudience));
        }
        let posting =
            self.protocol
                .read(cx)
                .post_status_text(text.clone(), background, &self.audience, cx);
        let me = self.protocol.read(cx).me().map(|me| me.phone);
        cx.spawn(async move |this, cx| {
            let id = posting.await?;
            let _ = this.update(cx, |book, _| {
                book.send(Change::Posted(Box::new(own_post(
                    me,
                    id,
                    ContentKind::Text,
                    Some(text),
                    Some(background),
                    None,
                ))));
            });
            Ok(())
        })
    }

    /// Posts the photo or video at `path` with `caption`.
    pub fn post_file(
        &mut self,
        path: PathBuf,
        caption: Option<String>,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), PostError>> {
        if self.audience.is_empty() {
            return Task::ready(Err(PostError::NoAudience));
        }
        let (audience, protocol) = (self.audience.clone(), self.protocol.clone());
        let me = self.protocol.read(cx).me().map(|me| me.phone);
        cx.spawn(async move |this, cx| {
            let file = path.clone();
            let caption_kept = caption.clone();
            let post = cx
                .background_spawn(async move { natizap_outbox::prepare(&file, caption, false) })
                .await?;
            let kind = post.kind;
            if !matches!(kind, ContentKind::Photo | ContentKind::Video) {
                return Err(PostError::NotPhotoOrVideo);
            }
            let posting = protocol.update(cx, |protocol, cx| {
                protocol.post_status_media(post, &audience, cx)
            });
            let id = posting.await?;
            let _ = this.update(cx, |book, _| {
                book.send(Change::Posted(Box::new(own_post(
                    me,
                    id,
                    kind,
                    caption_kept,
                    None,
                    Some(path),
                ))));
            });
            Ok(())
        })
    }

    fn send(&self, change: Change) {
        // The channel is unbounded and the worker lives as long as `self`.
        if self.inputs.try_send(Input::Change(change)).is_err() {
            tracing::error!("status worker stopped");
        }
    }

    /// Downloads `post`'s photo or video next, as it is on screen; one
    /// that failed is tried again, as a tap does on the phone.
    pub fn want(&mut self, post: &Post, cx: &mut Context<Self>) {
        if post.file.is_none() && post.media.is_some() {
            self.failed.remove(&post.id);
            self.wanted = Some(post.id.clone());
            self.fetch_next(cx);
        }
    }

    /// Downloads the next photo or video not downloaded yet, one at a time,
    /// the one on screen first.
    fn fetch_next(&mut self, cx: &mut Context<Self>) {
        if !self.fetching.is_empty() {
            return;
        }
        let missing = |post: &&Post| {
            post.file.is_none() && post.media.is_some() && !self.failed.contains(&post.id)
        };
        let wanted = self.wanted.as_ref().and_then(|wanted| {
            self.snapshot
                .posts
                .iter()
                .filter(missing)
                .find(|post| post.id == *wanted)
        });
        let Some(post) = wanted
            .or_else(|| self.snapshot.posts.iter().rev().find(missing))
            .cloned()
        else {
            return;
        };
        let Some(media) = post.media.clone() else {
            return;
        };
        self.fetching.insert(post.id.clone());
        let downloading = self.protocol.read(cx).download(post.kind, media, cx);
        let file = self.media_dir.join(file_name(&post));
        cx.spawn(async move |this, cx| {
            let saved = match downloading.await {
                Ok(bytes) => {
                    let file = file.clone();
                    cx.background_spawn(async move { save(&file, &bytes) })
                        .await
                }
                Err(error) => Err(error.to_string()),
            };
            let _ = this.update(cx, |book, cx| {
                book.fetching.remove(&post.id);
                match saved {
                    Ok(()) => book.send(Change::Downloaded {
                        poster: post.poster.clone(),
                        id: post.id.clone(),
                        file,
                    }),
                    Err(error) => {
                        tracing::warn!(%error, "could not download a status");
                        book.failed.insert(post.id.clone());
                    }
                }
                book.fetch_next(cx);
            });
        })
        .detach();
    }
}

/// This account's post `id`, as it is kept until WhatsApp echoes it.
fn own_post(
    me: Option<ChatId>,
    id: String,
    kind: ContentKind,
    text: Option<String>,
    background: Option<u32>,
    file: Option<PathBuf>,
) -> Post {
    Post {
        poster: me.unwrap_or_else(|| ChatId::new("me")),
        id,
        sent_at: Utc::now().timestamp(),
        kind,
        text,
        background,
        author: None,
        media: None,
        file,
        seen: true,
        mine: true,
        link: None,
    }
}

fn file_name(post: &Post) -> String {
    let id: String = post
        .id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    let extension = match post.kind {
        ContentKind::Video => "mp4",
        _ => "jpg",
    };
    format!("{id}.{extension}")
}

fn save(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    std::fs::write(path, bytes).map_err(|error| error.to_string())
}

/// Applies changes to the store and reads it back, off the UI thread.
async fn work(
    path: PathBuf,
    inputs: async_channel::Receiver<Input>,
    snapshots: async_channel::Sender<Snapshot>,
) {
    let opened = StatusStore::open(&path).or_else(|error| {
        tracing::error!(%error, "could not open the status store; keeping it in memory");
        StatusStore::in_memory()
    });
    let mut store = match opened {
        Ok(store) => store,
        Err(error) => {
            tracing::error!(%error, "no status store at all");
            return;
        }
    };
    loop {
        let read = read(&mut store);
        if snapshots.send(read).await.is_err() {
            return;
        }
        let Ok(input) = inputs.recv().await else {
            return;
        };
        let mut batch = expand(input);
        // Everything already queued goes in one transaction.
        while let Ok(more) = inputs.try_recv() {
            batch.extend(expand(more));
        }
        if let Err(error) = store.apply(&batch) {
            tracing::error!(%error, "could not keep status updates");
        }
    }
}

fn expand(input: Input) -> Vec<Change> {
    match input {
        Input::Event(event) => changes(&event),
        Input::Change(change) => vec![change],
    }
}

fn read(store: &mut StatusStore) -> Snapshot {
    let posts = store
        .recent(Utc::now().timestamp())
        .unwrap_or_else(|error| {
            tracing::error!(%error, "could not read status updates");
            Vec::new()
        });
    let viewers = store.viewers().unwrap_or_else(|error| {
        tracing::error!(%error, "could not read who saw your status");
        Vec::new()
    });
    Snapshot {
        posts: Arc::from(posts),
        viewers: Arc::from(viewers),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_downloaded_files_by_id_and_kind() {
        let post = own_post(None, "3EB0:ab".into(), ContentKind::Video, None, None, None);
        assert_eq!(file_name(&post), "3EB0ab.mp4");
    }
}
