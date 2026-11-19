//! Profile pictures of the people and groups natizap shows.
//!
//! [`ProfilePictures`] fetches a chat's picture the first time a view asks
//! for it, keeps it on disk so the next start shows it at once, and fetches
//! it again when its owner changes it. The chat list and the conversation
//! both draw these, so they live here rather than in either feature.

mod files;

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::Duration;

use gpui::{AppContext as _, Context, Entity, Task};
use natizap_protocol::whatsapp_rust::prelude::{Event, EventKind};
use natizap_protocol::{ActionError, ChatId, Protocol};

/// The rest between two picture lookups.
const FETCH_GAP: Duration = Duration::from_millis(400);
/// The rest after WhatsApp says it is asked too often.
const BUSY_REST: Duration = Duration::from_secs(30);

/// What is known of one chat's picture.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Known {
    /// On disk at this path.
    Picture(PathBuf),
    /// The chat has none, or hides it from this account.
    Missing,
    /// Asked for and not yet answered.
    Pending,
}

/// Every profile picture natizap has, and the ones it is fetching.
#[derive(Debug)]
pub struct ProfilePictures {
    protocol: Entity<Protocol>,
    dir: PathBuf,
    known: HashMap<ChatId, Known>,
    queue: VecDeque<ChatId>,
    fetching: bool,
    /// Whether the pictures on disk are known yet. Until then nothing is
    /// fetched, or every chat on the first screen would be asked of
    /// WhatsApp again although its picture is on disk.
    scanned: bool,
    _tasks: [Task<()>; 2],
}

impl ProfilePictures {
    /// Keeps pictures in `dir`, fetching them through `protocol`.
    pub fn new(protocol: Entity<Protocol>, dir: PathBuf, cx: &mut Context<Self>) -> Self {
        let scan_dir = dir.clone();
        let scan = cx.spawn(async move |this, cx| {
            let found = cx
                .background_spawn(async move { files::scan(&scan_dir) })
                .await;
            let _ = this.update(cx, |pictures, cx| {
                for (chat, path) in found {
                    pictures.known.entry(chat).or_insert(Known::Picture(path));
                }
                pictures.scanned = true;
                // Views ask again, and only what is not on disk is fetched.
                cx.notify();
            });
        });
        let changes = protocol
            .read(cx)
            .subscribe(&[EventKind::PictureUpdate, EventKind::Connected]);
        let follow = cx.spawn(async move |this, cx| {
            while let Ok(event) = changes.recv().await {
                let followed = this.update(cx, |pictures, cx| match event.as_ref() {
                    Event::PictureUpdate(update) => {
                        pictures.changed(ChatId::new(&update.jid.to_string()), cx);
                    }
                    Event::Connected(_) => pictures.connected(cx),
                    _ => {}
                });
                if followed.is_err() {
                    break;
                }
            }
        });
        Self {
            protocol,
            dir,
            known: HashMap::new(),
            queue: VecDeque::new(),
            fetching: false,
            scanned: false,
            _tasks: [scan, follow],
        }
    }

    /// The picture of `chat`, if natizap has one. The first time a chat is
    /// asked for, its picture is fetched; observers are notified when it
    /// arrives.
    pub fn picture(&mut self, chat: &ChatId, cx: &mut Context<Self>) -> Option<PathBuf> {
        match self.known.get(chat) {
            Some(Known::Picture(path)) => Some(path.clone()),
            Some(Known::Missing | Known::Pending) => None,
            None if !self.scanned => None,
            None => {
                self.known.insert(chat.clone(), Known::Pending);
                self.queue.push_back(chat.clone());
                self.fetch_next(cx);
                None
            }
        }
    }

    /// Fetches `chat`'s picture again, as after this account changed its
    /// own: WhatsApp does not always tell the device that changed it.
    pub fn refresh(&mut self, chat: &ChatId, cx: &mut Context<Self>) {
        self.known.remove(chat);
        self.picture(chat, cx);
        cx.notify();
    }

    /// Lookups made before the connection was up failed for that reason
    /// alone, such as your own picture asked for by the first screen: what
    /// is missing is asked for again.
    fn connected(&mut self, cx: &mut Context<Self>) {
        self.known.retain(|_, known| !matches!(known, Known::Missing));
        cx.notify();
    }

    /// Someone changed or removed their picture: fetch it again if it was
    /// on screen.
    fn changed(&mut self, chat: ChatId, cx: &mut Context<Self>) {
        if self.known.remove(&chat).is_some() {
            self.queue.push_back(chat);
            self.fetch_next(cx);
        }
    }

    /// Fetches one picture at a time, so a long chat list does not flood
    /// WhatsApp with requests.
    fn fetch_next(&mut self, cx: &mut Context<Self>) {
        if self.fetching {
            return;
        }
        let Some(chat) = self.queue.pop_front() else {
            return;
        };
        self.fetching = true;
        let fetching = self.protocol.read(cx).profile_picture(&chat, false, cx);
        let dir = self.dir.clone();
        cx.spawn(async move |this, cx| {
            let answer = fetching.await;
            // WhatsApp limits how often pictures may be asked for: a short
            // rest between them, and a long one once it says so.
            let busy = matches!(answer, Err(ActionError::Busy));
            let rest = if busy { BUSY_REST } else { FETCH_GAP };
            let known = match answer {
                Ok(Some(bytes)) => {
                    let saving_chat = chat.clone();
                    let saved = cx
                        .background_spawn(async move { files::save(&dir, &saving_chat, &bytes) })
                        .await;
                    match saved {
                        Ok(path) => Some(Known::Picture(path)),
                        Err(error) => {
                            tracing::warn!(%error, "could not save a profile picture");
                            Some(Known::Missing)
                        }
                    }
                }
                Ok(None) => Some(Known::Missing),
                // Asked again the next time the chat shows, once connected
                // or once WhatsApp takes questions again.
                Err(ActionError::Offline | ActionError::Interrupted | ActionError::Busy) => None,
                Err(error) => {
                    tracing::debug!(%error, chat = chat.as_str(), "no profile picture");
                    Some(Known::Missing)
                }
            };
            let _ = this.update(cx, |pictures, cx| {
                match known {
                    Some(known) => {
                        pictures.known.insert(chat, known);
                    }
                    // Still wanted, so asked again once the rest is over.
                    None if busy => pictures.queue.push_back(chat),
                    None => {
                        pictures.known.remove(&chat);
                    }
                }
                cx.notify();
            });
            cx.background_executor().timer(rest).await;
            let _ = this.update(cx, |pictures, cx| {
                pictures.fetching = false;
                pictures.fetch_next(cx);
            });
        })
        .detach();
    }
}
