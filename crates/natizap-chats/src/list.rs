use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::{App, AppContext as _, Context, Entity, SharedString, Task};
use natizap_protocol::Protocol;
use natizap_protocol::whatsapp_rust::prelude::Event;

use natizap_protocol::{ChatId, ContentKind, Disappearing, Marked, PersonName};

use crate::book::{Chat, ChatBook, ChatChange, Mute, Preview};
use crate::store::ChatStore;
use crate::sync::{CHAT_EVENTS, changes};

/// Time left between background business lookups, so they stay under
/// the contacts service's rate limit.
const BUSINESS_LOOKUP_GAP: Duration = Duration::from_secs(5);

/// One line of the chat list, ready to render.
#[derive(Debug, Clone)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "marked unread, pinned, muted and verified are independent facts \
              the list shows per chat"
)]
pub struct ChatRow {
    /// The chat's address.
    pub id: ChatId,
    /// Every address its messages may be filed under, `id` first.
    pub addresses: Arc<[ChatId]>,
    /// Name to show.
    pub title: SharedString,
    /// The latest message, prefixed with its author in groups.
    pub preview: Option<SharedString>,
    /// What the latest message was, when not text: a photo, a sticker…
    pub preview_kind: Option<ContentKind>,
    /// When the latest message was sent, in Unix seconds.
    pub sent_at: Option<i64>,
    /// Unread messages.
    pub unread: u32,
    /// Marked as unread by hand.
    pub marked_unread: bool,
    /// Pinned to the top.
    pub pinned: bool,
    /// Notifications silenced.
    pub muted: bool,
    /// A business WhatsApp vouches for, shown with a seal.
    pub verified: bool,
    /// WhatsApp was already asked whether it is a business.
    pub(crate) looked_up: bool,
    /// Known to be a business, seal or not.
    pub(crate) business: bool,
    /// How long new messages last, when they disappear.
    pub disappearing: Option<Disappearing>,
}

impl ChatRow {
    /// Whether the chat answers a search for `query`: its name holds it,
    /// ignoring case, or, for a number, its phone number holds the digits.
    #[must_use]
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        let digits: String = query.chars().filter(char::is_ascii_digit).collect();
        let number = digits.len() >= 3
            && query
                .chars()
                .all(|c| c.is_ascii_digit() || " +-()".contains(c));
        let by_number = number
            && self.addresses.iter().any(|address| {
                address.phone_number().is_some()
                    && address
                        .as_str()
                        .split('@')
                        .next()
                        .is_some_and(|user| user.contains(&digits))
            });
        query.is_empty() || self.title.to_lowercase().contains(&query) || by_number
    }

    fn new(book: &ChatBook, chat: &Chat, aliases: &[&ChatId], now: i64) -> Self {
        let preview = chat.preview.as_ref().map(|preview| {
            // Media without a caption reads as what it is.
            let said = if preview.text.is_empty() {
                preview.kind.label()
            } else {
                &preview.text
            };
            let text = match (&preview.author, preview.from_me) {
                (_, true) => format!("You: {said}"),
                (Some(author), false) => format!("{author}: {said}"),
                (None, false) => said.to_owned(),
            };
            let text = Marked::parse(&text).text;
            SharedString::from(first_line(&text).to_owned())
        });
        Self {
            id: chat.id.clone(),
            addresses: std::iter::once(&chat.id)
                .chain(aliases.iter().copied())
                .cloned()
                .collect(),
            title: book.title(chat, aliases).into(),
            preview,
            preview_kind: chat
                .preview
                .as_ref()
                .map(|preview| preview.kind)
                .filter(|kind| *kind != ContentKind::Text),
            sent_at: chat.preview.as_ref().map(|preview| preview.sent_at),
            unread: chat.unread,
            marked_unread: chat.marked_unread,
            pinned: chat.pinned_at.is_some(),
            muted: chat.is_muted(now),
            verified: book.verified(chat, aliases),
            looked_up: book.looked_up(chat, aliases),
            business: book.business(chat, aliases),
            disappearing: chat.disappearing.and_then(Disappearing::from_seconds),
        }
    }
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or_default()
}

#[derive(Debug, Clone, Default)]
struct Snapshot {
    rows: Arc<[ChatRow]>,
    archived: Arc<[ChatRow]>,
    names: Arc<HashMap<ChatId, PersonName>>,
}

/// Requests to the sync worker, alongside protocol events.
enum Input {
    Event(Arc<Event>),
    Open(Option<ChatId>),
    Change(ChatChange),
}

/// The user's chats, kept in sync with WhatsApp and cached on disk.
///
/// Opens the cache immediately, so the list shows before the connection is
/// up, then follows history syncs and live updates. Notifies observers when
/// the list changes.
#[derive(Debug)]
pub struct ChatList {
    protocol: Entity<Protocol>,
    /// Chats already asked whether they are businesses.
    asked: HashSet<ChatId>,
    /// Whether a question is in flight.
    asking: bool,
    snapshot: Snapshot,
    inputs: async_channel::Sender<Input>,
    _tasks: [Task<()>; 3],
}

impl ChatList {
    /// Follows `protocol`'s chat events, caching the list at `cache_path`.
    pub fn new(protocol: &Entity<Protocol>, cache_path: PathBuf, cx: &mut Context<Self>) -> Self {
        let (inputs_tx, inputs) = async_channel::unbounded();
        let events = protocol.read(cx).subscribe(CHAT_EVENTS);
        let forward_tx = inputs_tx.clone();
        let forward = cx.background_spawn(async move {
            while let Ok(event) = events.recv().await {
                if forward_tx.send(Input::Event(event)).await.is_err() {
                    break;
                }
            }
        });
        // Read on the phone: what arrived meanwhile is not new any more.
        let reads = protocol.read(cx).reads_elsewhere(cx);
        let reads_tx = inputs_tx.clone();
        cx.background_spawn(async move {
            while let Ok(read) = reads.recv().await {
                let change = ChatChange::Read(read.chat, true);
                if reads_tx.send(Input::Change(change)).await.is_err() {
                    break;
                }
            }
        })
        .detach();
        let (snapshots_tx, snapshots) = async_channel::unbounded();
        let sync = cx.background_spawn(sync(cache_path, inputs, snapshots_tx));
        let apply = cx.spawn(async move |this, cx| {
            while let Ok(mut snapshot) = snapshots.recv().await {
                // Only the newest list matters if several queued up.
                while let Ok(newer) = snapshots.try_recv() {
                    snapshot = newer;
                }
                let applied = this.update(cx, |list, cx| {
                    list.snapshot = list.mark_own(snapshot, cx);
                    list.name_businesses(cx);
                    cx.notify();
                });
                if applied.is_err() {
                    break;
                }
            }
        });
        Self {
            protocol: protocol.clone(),
            asked: HashSet::new(),
            asking: false,
            snapshot: Snapshot::default(),
            inputs: inputs_tx,
            _tasks: [forward, sync, apply],
        }
    }

    /// Chats in display order; archived chats are left out.
    #[must_use]
    pub fn rows(&self) -> &Arc<[ChatRow]> {
        &self.snapshot.rows
    }

    /// What every chat is called, archived ones included, by every address
    /// it goes by.
    #[must_use]
    pub fn titles(&self) -> HashMap<ChatId, SharedString> {
        self.rows()
            .iter()
            .chain(self.archived().iter())
            .flat_map(|row| {
                row.addresses
                    .iter()
                    .chain([&row.id])
                    .map(|address| (address.clone(), row.title.clone()))
            })
            .collect()
    }

    /// Everyone's names, by each address they are known under, people
    /// without a chat included. The same `Arc` until a name changes.
    #[must_use]
    pub fn names(&self) -> &Arc<HashMap<ChatId, PersonName>> {
        &self.snapshot.names
    }

    /// Archived chats, in display order.
    #[must_use]
    pub fn archived(&self) -> &Arc<[ChatRow]> {
        &self.snapshot.archived
    }

    /// Marks `id` as the chat on screen, reading it; messages arriving there
    /// while it stays open are read too.
    pub fn open(&self, id: Option<ChatId>) {
        if let Some(id) = &id {
            self.send(Input::Change(ChatChange::Read(id.clone(), true)));
        }
        self.send(Input::Open(id));
    }

    /// Reflects a message this computer sent in `chat`, before WhatsApp
    /// echoes it back: what it was, and its text or caption (empty for
    /// none).
    pub fn record_sent(&self, chat: ChatId, kind: ContentKind, text: String, sent_at: i64) {
        let preview = Preview {
            kind,
            text,
            sent_at,
            from_me: true,
            author: None,
        };
        self.send(Input::Change(ChatChange::Message(chat, preview)));
    }

    /// Mutes `chat` until unmuted, or unmutes it, here and on the account's
    /// other devices.
    pub fn set_muted(&self, chat: &ChatId, muted: bool, cx: &App) {
        let mute = muted.then_some(Mute::Forever);
        self.send(Input::Change(ChatChange::Muted(chat.clone(), mute)));
        let muting = self.protocol.read(cx).mute(chat, muted, cx);
        cx.background_spawn(async move {
            if let Err(error) = muting.await {
                tracing::warn!(%error, "mute not synced to the other devices");
            }
        })
        .detach();
    }

    /// Makes new messages in `chat` disappear after `timer`, or stops it,
    /// for everyone in the chat.
    pub fn set_disappearing(&self, chat: &ChatId, timer: Option<Disappearing>, cx: &App) {
        let seconds = timer.map(Disappearing::seconds);
        self.send(Input::Change(ChatChange::Disappearing(
            chat.clone(),
            seconds,
        )));
        let setting = self.protocol.read(cx).set_disappearing(chat, timer, cx);
        cx.background_spawn(async move {
            if let Err(error) = setting.await {
                tracing::warn!(%error, "disappearing messages not changed");
            }
        })
        .detach();
    }

    /// Names the account's own chat as the phone does, "+55 … (You)".
    fn mark_own(&self, mut snapshot: Snapshot, cx: &App) -> Snapshot {
        let protocol = self.protocol.read(cx);
        let own = |row: &ChatRow| row.addresses.iter().any(|address| protocol.is_own(address));
        if snapshot.rows.iter().any(own) {
            snapshot.rows = snapshot
                .rows
                .iter()
                .map(|row| {
                    let mut row = row.clone();
                    if own(&row) {
                        row.title = format!("{} (You)", row.title).into();
                    }
                    row
                })
                .collect();
        }
        snapshot
    }

    /// Asks, one at a time, whether chats known only by their number are
    /// businesses, and whether businesses without a seal have one: chats
    /// from before natizap read business names from messages have neither.
    /// Each chat is asked once, ever; the answer is kept.
    fn name_businesses(&mut self, cx: &mut Context<Self>) {
        if self.asking {
            return;
        }
        let unnamed = self.snapshot.rows.iter().find(|row| {
            let unnamed = row
                .addresses
                .iter()
                .any(|address| address.phone_number().as_deref() == Some(&*row.title));
            !row.id.is_group()
                && !row.looked_up
                && !self.asked.contains(&row.id)
                && (unnamed || (row.business && !row.verified))
        });
        let Some(row) = unnamed else {
            return;
        };
        self.asked.insert(row.id.clone());
        let (id, phone) = (
            row.id.clone(),
            row.addresses
                .iter()
                .find(|address| address.phone_number().is_some())
                .cloned()
                .unwrap_or_else(|| row.id.clone()),
        );
        let asking = self.protocol.read(cx).profile(&phone, cx);
        self.asking = true;
        cx.spawn(async move |this, cx| {
            let answer = asking.await;
            // WhatsApp's contacts service refuses a burst of lookups
            // (`rate-overlimit`), and then refuses what the user opens too:
            // these background ones leave room between them.
            cx.background_executor().timer(BUSINESS_LOOKUP_GAP).await;
            let _ = this.update(cx, |list, cx| {
                let profile = match answer {
                    Ok(profile) => profile,
                    // Refused, most likely for asking too often: no more
                    // lookups this run, and this chat is asked next run.
                    Err(error) => {
                        tracing::debug!(%error, "business lookups paused until next launch");
                        return;
                    }
                };
                let named = profile
                    .business
                    .and_then(|business| Some((business.name?, business.verified)));
                if let Some((name, verified)) = named {
                    let change = ChatChange::BusinessName(id.clone(), name, Some(verified));
                    list.send(Input::Change(change));
                }
                list.send(Input::Change(ChatChange::LookedUp(id)));
                list.asking = false;
                list.name_businesses(cx);
            });
        })
        .detach();
    }

    fn send(&self, input: Input) {
        // The channel is unbounded and the worker lives as long as `self`.
        if self.inputs.try_send(input).is_err() {
            tracing::error!("chat list worker stopped");
        }
    }
}

/// Folds protocol events and requests into the book and the cache, off the
/// UI thread, and sends a fresh snapshot after each burst.
async fn sync(
    cache_path: PathBuf,
    inputs: async_channel::Receiver<Input>,
    snapshots: async_channel::Sender<Snapshot>,
) {
    let Some(mut store) = open_store(&cache_path) else {
        return;
    };
    let mut book = store.load().unwrap_or_else(|error| {
        tracing::error!(%error, "could not read the chat cache; starting empty");
        ChatBook::default()
    });
    let mut names = Arc::new(book.names());
    if snapshots.send(snapshot(&book, &names)).await.is_err() {
        return;
    }

    while let Ok(input) = inputs.recv().await {
        handle(&mut book, &mut store, input);
        while let Ok(input) = inputs.try_recv() {
            handle(&mut book, &mut store, input);
        }
        let dirty = book.take_dirty();
        if let Err(error) = store.save(&book, &dirty) {
            tracing::error!(%error, "could not update the chat cache");
        }
        if !dirty.people.is_empty() {
            names = Arc::new(book.names());
        }
        if snapshots.send(snapshot(&book, &names)).await.is_err() {
            return;
        }
    }
}

fn handle(book: &mut ChatBook, store: &mut ChatStore, input: Input) {
    match input {
        Input::Event(event) if matches!(*event, Event::LoggedOut(_)) => {
            // Whoever links next may be a different account.
            *book = ChatBook::default();
            if let Err(error) = store.clear() {
                tracing::error!(%error, "could not clear the chat cache");
            }
        }
        Input::Event(event) => {
            for change in changes(&event) {
                book.apply(change);
            }
        }
        Input::Open(id) => book.set_open(id.as_ref()),
        Input::Change(change) => book.apply(change),
    }
}

fn open_store(path: &Path) -> Option<ChatStore> {
    ChatStore::open(path)
        .or_else(|error| {
            // The list still works without a cache; it just starts empty
            // next time.
            tracing::error!(%error, path = %path.display(), "could not open the chat cache");
            ChatStore::in_memory()
        })
        .inspect_err(|error| tracing::error!(%error, "could not create an in-memory chat cache"))
        .ok()
}

fn snapshot(book: &ChatBook, names: &Arc<HashMap<ChatId, PersonName>>) -> Snapshot {
    let now = unix_now();
    let addresses = book.addresses();
    let rows = |chats: Vec<&Chat>| -> Arc<[ChatRow]> {
        chats
            .into_iter()
            .map(|chat| {
                let aliases = addresses.get(&chat.id).map_or(&[][..], Vec::as_slice);
                ChatRow::new(book, chat, aliases, now)
            })
            .collect()
    };
    Snapshot {
        rows: rows(book.listed()),
        archived: rows(book.archived()),
        names: names.clone(),
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}
