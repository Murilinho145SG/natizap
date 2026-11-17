use std::collections::{HashMap, HashSet};

use natizap_protocol::{ChatId, PersonName};

/// The latest message of a chat, as shown under its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Preview {
    /// Text or a short description of the content, e.g. "Photo".
    pub(crate) text: String,
    /// When it was sent, in Unix seconds.
    pub(crate) sent_at: i64,
    /// Whether this account sent it.
    pub(crate) from_me: bool,
    /// Who sent it, for group chats.
    pub(crate) author: Option<String>,
}

/// How long a chat stays muted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mute {
    /// Until unmuted by hand.
    Forever,
    /// Until this Unix time, in seconds.
    Until(i64),
}

/// A chat as the list knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Chat {
    /// The chat's address.
    pub(crate) id: ChatId,
    /// Group subject, or the name WhatsApp attached to the chat.
    pub(crate) subject: Option<String>,
    /// The latest message.
    pub(crate) preview: Option<Preview>,
    /// Latest activity, in Unix seconds; orders the list.
    pub(crate) active_at: i64,
    /// Messages received since the chat was last read.
    pub(crate) unread: u32,
    /// Marked as unread by hand, independently of `unread`.
    pub(crate) marked_unread: bool,
    /// When the chat was pinned, in Unix seconds.
    pub(crate) pinned_at: Option<i64>,
    /// Whether, and until when, notifications are silenced.
    pub(crate) mute: Option<Mute>,
    /// Hidden in the archive.
    pub(crate) archived: bool,
    /// Seconds new messages last before disappearing, when they do.
    pub(crate) disappearing: Option<u32>,
}

impl Chat {
    fn new(id: ChatId) -> Self {
        Self {
            id,
            subject: None,
            preview: None,
            active_at: 0,
            unread: 0,
            marked_unread: false,
            pinned_at: None,
            mute: None,
            archived: false,
            disappearing: None,
        }
    }

    /// Whether notifications are silenced at Unix time `now`.
    #[must_use]
    pub(crate) fn is_muted(&self, now: i64) -> bool {
        match self.mute {
            Some(Mute::Forever) => true,
            Some(Mute::Until(until)) => until > now,
            None => false,
        }
    }

    fn show(&mut self, preview: Preview) {
        self.active_at = self.active_at.max(preview.sent_at);
        if self
            .preview
            .as_ref()
            .is_none_or(|current| current.sent_at <= preview.sent_at)
        {
            self.preview = Some(preview);
        }
    }

    fn absorb(&mut self, other: Chat) {
        self.subject = self.subject.take().or(other.subject);
        if let Some(preview) = other.preview {
            self.show(preview);
        }
        self.active_at = self.active_at.max(other.active_at);
        self.unread = self.unread.max(other.unread);
        self.marked_unread |= other.marked_unread;
        self.pinned_at = self.pinned_at.max(other.pinned_at);
        self.mute = self.mute.or(other.mute);
        self.archived &= other.archived;
    }
}

/// What a person is called. Kept apart from chats: names arrive for
/// everyone in the address book, not only for people with a chat.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Person {
    /// Name saved in the phone's address book.
    pub(crate) contact_name: Option<String>,
    /// Name the person chose for themselves.
    pub(crate) push_name: Option<String>,
    /// For a business account, the name WhatsApp verified for it.
    pub(crate) business_name: Option<String>,
    /// WhatsApp vouches for the business, which then shows a seal.
    pub(crate) verified: bool,
}

/// Snapshot of one chat from a history sync. History can arrive after live
/// messages, so it never replaces newer information.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct HistoryChat {
    /// Group subject or WhatsApp's name for the chat.
    pub(crate) name: Option<String>,
    /// The newest message in the snapshot.
    pub(crate) preview: Option<Preview>,
    /// Latest activity, in Unix seconds.
    pub(crate) active_at: i64,
    /// Unread messages at snapshot time.
    pub(crate) unread: u32,
    /// Marked as unread by hand.
    pub(crate) marked_unread: bool,
    /// When the chat was pinned.
    pub(crate) pinned_at: Option<i64>,
    /// Mute state.
    pub(crate) mute: Option<Mute>,
    /// Archived.
    pub(crate) archived: bool,
    /// Seconds new messages last, when they disappear.
    pub(crate) disappearing: Option<u32>,
}

/// One fact learned about the chats, from history or live updates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ChatChange {
    /// A chat as seen in a history sync.
    History(ChatId, HistoryChat),
    /// A new message arrived or was sent.
    Message(ChatId, Preview),
    /// Two JIDs address the same chat: WhatsApp moves 1:1 chats from phone
    /// numbers to LIDs and still uses both.
    SameChat(ChatId, ChatId),
    /// Name from the phone's address book.
    ContactName(ChatId, String),
    /// Name the person chose for themselves.
    PushName(ChatId, String),
    /// A business account's verified name, and whether WhatsApp vouches for
    /// it with a seal, when the source says.
    BusinessName(ChatId, String, Option<bool>),
    /// Pinned at the given time, or unpinned.
    Pinned(ChatId, Option<i64>),
    /// Muted, or unmuted.
    Muted(ChatId, Option<Mute>),
    /// New messages disappear after this many seconds, or stay.
    Disappearing(ChatId, Option<u32>),
    /// Archived or unarchived.
    Archived(ChatId, bool),
    /// Read on another device (`true`) or marked as unread (`false`).
    Read(ChatId, bool),
    /// Messages cleared; the chat stays.
    Cleared(ChatId),
    /// The chat was deleted.
    Deleted(ChatId),
}

/// What changed since the last [`ChatBook::take_dirty`], for persistence.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Dirty {
    /// Chats added, changed or removed; look each up to tell which.
    pub(crate) chats: HashSet<ChatId>,
    /// New `(alias, chat)` pairs.
    pub(crate) aliases: Vec<(ChatId, ChatId)>,
    /// People whose names changed.
    pub(crate) people: HashSet<ChatId>,
}

/// Every known chat, and the rules for folding changes into them.
#[derive(Debug, Default)]
pub(crate) struct ChatBook {
    chats: HashMap<ChatId, Chat>,
    aliases: HashMap<ChatId, ChatId>,
    /// Names by the address they arrived under; a chat finds them through any
    /// of its addresses.
    people: HashMap<ChatId, Person>,
    /// The chat on screen: what arrives there is read as it arrives.
    open: Option<ChatId>,
    dirty: Dirty,
}

impl ChatBook {
    /// Rebuilds a book from persisted chats, aliases and people.
    pub(crate) fn restore(
        chats: impl IntoIterator<Item = Chat>,
        aliases: impl IntoIterator<Item = (ChatId, ChatId)>,
        people: impl IntoIterator<Item = (ChatId, Person)>,
    ) -> Self {
        Self {
            chats: chats
                .into_iter()
                .map(|chat| (chat.id.clone(), chat))
                .collect(),
            aliases: aliases.into_iter().collect(),
            people: people.into_iter().collect(),
            open: None,
            dirty: Dirty::default(),
        }
    }

    /// Looks up a chat by any of its JIDs.
    #[must_use]
    pub(crate) fn get(&self, id: &ChatId) -> Option<&Chat> {
        self.chats.get(self.resolve(id))
    }

    /// Folds one change into the book.
    pub(crate) fn apply(&mut self, change: ChatChange) {
        match change {
            ChatChange::SameChat(a, b) => self.link(&a, &b),
            ChatChange::Deleted(id) => {
                let id = self.resolve(&id).clone();
                self.chats.remove(&id);
                self.dirty.chats.insert(id);
            }
            ChatChange::History(id, history) => self.update(&id, |chat| {
                chat.subject = chat.subject.take().or(history.name);
                if let Some(preview) = history.preview {
                    chat.show(preview);
                }
                chat.active_at = chat.active_at.max(history.active_at);
                chat.unread = chat.unread.max(history.unread);
                chat.marked_unread |= history.marked_unread;
                chat.pinned_at = chat.pinned_at.or(history.pinned_at);
                chat.mute = chat.mute.or(history.mute);
                chat.archived |= history.archived;
                chat.disappearing = chat.disappearing.or(history.disappearing);
            }),
            ChatChange::Message(id, preview) => {
                let on_screen = self.open.as_ref() == Some(self.resolve(&id));
                self.update(&id, |chat| {
                    if preview.from_me || on_screen {
                        // Replying from any device, or reading as it arrives,
                        // reads the chat, as on mobile.
                        chat.unread = 0;
                        chat.marked_unread = false;
                    } else {
                        chat.unread += 1;
                    }
                    chat.show(preview);
                });
            }
            ChatChange::ContactName(id, name) => {
                self.person(id).contact_name = Some(name);
            }
            ChatChange::PushName(id, name) => self.person(id).push_name = Some(name),
            ChatChange::BusinessName(id, name, verified) => {
                let person = self.person(id);
                person.business_name = Some(name);
                if let Some(verified) = verified {
                    person.verified = verified;
                }
            }
            ChatChange::Pinned(id, at) => self.update(&id, |chat| chat.pinned_at = at),
            ChatChange::Muted(id, mute) => self.update(&id, |chat| chat.mute = mute),
            ChatChange::Disappearing(id, seconds) => {
                self.update(&id, |chat| chat.disappearing = seconds);
            }
            ChatChange::Archived(id, archived) => self.update(&id, |chat| chat.archived = archived),
            ChatChange::Read(id, read) => self.update(&id, |chat| {
                if read {
                    chat.unread = 0;
                }
                chat.marked_unread = !read;
            }),
            ChatChange::Cleared(id) => self.update(&id, |chat| chat.preview = None),
        }
    }

    /// The chat list as shown: pinned chats first, most recently pinned on
    /// top, then the rest by latest activity. Archived chats are left out.
    #[must_use]
    pub(crate) fn listed(&self) -> Vec<&Chat> {
        self.ordered(false)
    }

    /// The archived chats, in the list's order.
    #[must_use]
    pub(crate) fn archived(&self) -> Vec<&Chat> {
        self.ordered(true)
    }

    fn ordered(&self, archived: bool) -> Vec<&Chat> {
        let mut chats: Vec<&Chat> = self
            .chats
            .values()
            .filter(|chat| chat.archived == archived)
            .collect();
        chats.sort_by(|a, b| {
            b.pinned_at
                .cmp(&a.pinned_at)
                .then(b.active_at.cmp(&a.active_at))
                .then_with(|| a.id.cmp(&b.id))
        });
        chats
    }

    /// Every address of each chat that has more than one, keyed by the
    /// address the chat is listed under.
    pub(crate) fn addresses(&self) -> HashMap<&ChatId, Vec<&ChatId>> {
        let mut addresses: HashMap<&ChatId, Vec<&ChatId>> = HashMap::new();
        for (alias, chat) in &self.aliases {
            addresses.entry(chat).or_default().push(alias);
        }
        addresses
    }

    /// The name to show for `chat`, given its other addresses: the address
    /// book wins, as on the phone, then the chat's own name, a business's
    /// verified name, the person's self-chosen name, and the phone number.
    #[must_use]
    pub(crate) fn title(&self, chat: &Chat, aliases: &[&ChatId]) -> String {
        let addresses = || std::iter::once(&chat.id).chain(aliases.iter().copied());
        let people = || addresses().filter_map(|address| self.people.get(address));
        people()
            .find_map(|person| person.contact_name.clone())
            .or_else(|| chat.subject.clone())
            .or_else(|| people().find_map(|person| person.business_name.clone()))
            .or_else(|| {
                people()
                    .find_map(|person| person.push_name.as_ref())
                    .map(|name| format!("~ {name}"))
            })
            .or_else(|| addresses().find_map(ChatId::phone_number))
            .unwrap_or_else(|| "Unknown contact".to_owned())
    }

    /// Whether `chat` is known to be a business.
    #[must_use]
    pub(crate) fn business(&self, chat: &Chat, aliases: &[&ChatId]) -> bool {
        std::iter::once(&chat.id)
            .chain(aliases.iter().copied())
            .filter_map(|address| self.people.get(address))
            .any(|person| person.business_name.is_some())
    }

    /// Whether `chat` is a business WhatsApp vouches for.
    #[must_use]
    pub(crate) fn verified(&self, chat: &Chat, aliases: &[&ChatId]) -> bool {
        std::iter::once(&chat.id)
            .chain(aliases.iter().copied())
            .filter_map(|address| self.people.get(address))
            .any(|person| person.verified)
    }

    /// Everyone's names, by each address they are known under.
    #[must_use]
    pub(crate) fn names(&self) -> HashMap<ChatId, PersonName> {
        self.people
            .iter()
            .filter(|(_, person)| person.contact_name.is_some() || person.push_name.is_some())
            .map(|(id, person)| {
                let name = PersonName {
                    saved: person.contact_name.clone(),
                    chosen: person.push_name.clone(),
                };
                (id.clone(), name)
            })
            .collect()
    }

    /// A person's names, for persistence.
    pub(crate) fn person_named(&self, id: &ChatId) -> Option<&Person> {
        self.people.get(id)
    }

    /// Marks `id` as the chat on screen, or none.
    pub(crate) fn set_open(&mut self, id: Option<&ChatId>) {
        self.open = id.map(|id| self.resolve(id).clone());
    }

    /// Returns and forgets what changed since the previous call.
    pub(crate) fn take_dirty(&mut self) -> Dirty {
        std::mem::take(&mut self.dirty)
    }

    fn person(&mut self, id: ChatId) -> &mut Person {
        self.dirty.people.insert(id.clone());
        self.people.entry(id).or_default()
    }

    fn resolve<'a>(&'a self, id: &'a ChatId) -> &'a ChatId {
        self.aliases.get(id).unwrap_or(id)
    }

    /// Applies `f` to the chat, creating it on first sight. Changes for chats
    /// that do not belong in the list are dropped.
    fn update(&mut self, id: &ChatId, f: impl FnOnce(&mut Chat)) {
        let id = self.resolve(id).clone();
        if !id.is_conversation() {
            return;
        }
        f(self
            .chats
            .entry(id.clone())
            .or_insert_with(|| Chat::new(id.clone())));
        self.dirty.chats.insert(id);
    }

    /// Records that `a` and `b` are the same chat, keeping whichever already
    /// exists and merging the two when both do.
    fn link(&mut self, a: &ChatId, b: &ChatId) {
        let a = self.resolve(a).clone();
        let b = self.resolve(b).clone();
        if a == b {
            return;
        }
        let (keep, alias) = if self.chats.contains_key(&b) && !self.chats.contains_key(&a) {
            (b, a)
        } else {
            (a, b)
        };
        if let Some(merged) = self.chats.remove(&alias) {
            self.chats
                .entry(keep.clone())
                .or_insert_with(|| Chat::new(keep.clone()))
                .absorb(merged);
            self.dirty.chats.insert(alias.clone());
            self.dirty.chats.insert(keep.clone());
        }
        for target in self.aliases.values_mut().filter(|target| **target == alias) {
            target.clone_from(&keep);
            self.dirty.aliases.push((alias.clone(), keep.clone()));
        }
        self.aliases.insert(alias.clone(), keep.clone());
        self.dirty.aliases.push((alias, keep));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(jid: &str) -> ChatId {
        ChatId::new(jid)
    }

    fn message(text: &str, sent_at: i64, from_me: bool) -> Preview {
        Preview {
            text: text.into(),
            sent_at,
            from_me,
            author: None,
        }
    }

    fn order(book: &ChatBook) -> Vec<&str> {
        book.listed().iter().map(|chat| chat.id.as_str()).collect()
    }

    #[test]
    fn lists_pinned_chats_first_then_by_latest_activity() {
        let mut book = ChatBook::default();
        book.apply(ChatChange::Message(
            id("a@s.whatsapp.net"),
            message("old", 10, false),
        ));
        book.apply(ChatChange::Message(
            id("b@s.whatsapp.net"),
            message("new", 20, false),
        ));
        book.apply(ChatChange::Message(
            id("c@s.whatsapp.net"),
            message("older", 5, false),
        ));
        book.apply(ChatChange::Pinned(id("c@s.whatsapp.net"), Some(100)));

        assert_eq!(
            order(&book),
            ["c@s.whatsapp.net", "b@s.whatsapp.net", "a@s.whatsapp.net"]
        );
    }

    #[test]
    fn archived_chats_leave_the_list_and_are_counted() {
        let mut book = ChatBook::default();
        book.apply(ChatChange::Message(
            id("a@s.whatsapp.net"),
            message("hi", 1, false),
        ));
        book.apply(ChatChange::Archived(id("a@s.whatsapp.net"), true));

        assert_eq!(book.listed().len(), 0);
        assert_eq!(book.archived().len(), 1);
    }

    #[test]
    fn incoming_messages_count_as_unread_until_a_reply() {
        let mut book = ChatBook::default();
        let chat = id("a@s.whatsapp.net");
        book.apply(ChatChange::Message(chat.clone(), message("one", 1, false)));
        book.apply(ChatChange::Message(chat.clone(), message("two", 2, false)));
        assert_eq!(book.get(&chat).map(|c| c.unread), Some(2));

        book.apply(ChatChange::Message(chat.clone(), message("reply", 3, true)));
        assert_eq!(book.get(&chat).map(|c| c.unread), Some(0));
    }

    #[test]
    fn messages_arriving_in_the_open_chat_are_read() {
        let mut book = ChatBook::default();
        let chat = id("a@s.whatsapp.net");
        book.set_open(Some(&chat));
        book.apply(ChatChange::Message(chat.clone(), message("hi", 1, false)));
        assert_eq!(book.get(&chat).map(|c| c.unread), Some(0));

        book.set_open(None);
        book.apply(ChatChange::Message(
            chat.clone(),
            message("again", 2, false),
        ));
        assert_eq!(book.get(&chat).map(|c| c.unread), Some(1));
    }

    #[test]
    fn late_history_does_not_replace_a_newer_preview() {
        let mut book = ChatBook::default();
        let chat = id("a@s.whatsapp.net");
        book.apply(ChatChange::Message(
            chat.clone(),
            message("live", 50, false),
        ));
        book.apply(ChatChange::History(
            chat.clone(),
            HistoryChat {
                preview: Some(message("history", 10, false)),
                ..Default::default()
            },
        ));

        let preview = book
            .get(&chat)
            .and_then(|c| c.preview.as_ref())
            .map(|p| p.text.as_str());
        assert_eq!(preview, Some("live"));
    }

    #[test]
    fn phone_number_and_lid_merge_into_one_chat() {
        let mut book = ChatBook::default();
        let phone = id("5511999@s.whatsapp.net");
        let lid = id("123456@lid");
        book.apply(ChatChange::Message(
            phone.clone(),
            message("by phone", 1, false),
        ));
        book.apply(ChatChange::Message(
            lid.clone(),
            message("by lid", 2, false),
        ));
        book.apply(ChatChange::SameChat(lid.clone(), phone.clone()));

        assert_eq!(book.listed().len(), 1);
        let merged = book
            .get(&phone)
            .map(|c| (c.preview.clone().map(|p| p.text), c.unread));
        assert_eq!(merged, Some((Some("by lid".into()), 1)));
        assert_eq!(
            book.get(&lid).map(|c| &c.id),
            book.get(&phone).map(|c| &c.id)
        );

        book.apply(ChatChange::Message(lid, message("later", 3, false)));
        assert_eq!(book.listed().len(), 1);
    }

    #[test]
    fn status_updates_and_channels_stay_out_of_the_list() {
        let mut book = ChatBook::default();
        book.apply(ChatChange::Message(
            id("status@broadcast"),
            message("story", 1, false),
        ));
        book.apply(ChatChange::Message(
            id("1@newsletter"),
            message("post", 1, false),
        ));

        assert_eq!(book.listed().len(), 0);
    }

    fn title(book: &ChatBook, chat: &ChatId) -> Option<String> {
        let addresses = book.addresses();
        let chat = book.get(chat)?;
        let aliases = addresses.get(&chat.id).map_or(&[][..], Vec::as_slice);
        Some(book.title(chat, aliases))
    }

    #[test]
    fn title_prefers_the_address_book_then_falls_back_to_the_number() {
        let mut book = ChatBook::default();
        let chat = id("5511999@s.whatsapp.net");
        book.apply(ChatChange::Message(chat.clone(), message("hi", 1, false)));
        assert_eq!(title(&book, &chat).as_deref(), Some("+5511999"));

        book.apply(ChatChange::PushName(chat.clone(), "Ana".into()));
        assert_eq!(title(&book, &chat).as_deref(), Some("~ Ana"));

        book.apply(ChatChange::ContactName(chat.clone(), "Ana Souza".into()));
        assert_eq!(title(&book, &chat).as_deref(), Some("Ana Souza"));
    }

    #[test]
    fn a_lid_chat_finds_names_saved_under_the_phone_number() {
        let mut book = ChatBook::default();
        let lid = id("123@lid");
        book.apply(ChatChange::Message(lid.clone(), message("hi", 1, false)));
        assert_eq!(title(&book, &lid).as_deref(), Some("Unknown contact"));

        book.apply(ChatChange::ContactName(
            id("5511999@s.whatsapp.net"),
            "Ana".into(),
        ));
        book.apply(ChatChange::SameChat(
            lid.clone(),
            id("5511999@s.whatsapp.net"),
        ));
        assert_eq!(title(&book, &lid).as_deref(), Some("Ana"));
    }

    #[test]
    fn names_alone_do_not_create_chats() {
        let mut book = ChatBook::default();
        book.apply(ChatChange::PushName(id("1@lid"), "Ana".into()));
        book.apply(ChatChange::ContactName(
            id("2@s.whatsapp.net"),
            "Bruno".into(),
        ));

        assert_eq!(book.listed().len(), 0);
        assert_eq!(book.take_dirty().people.len(), 2);
    }

    #[test]
    fn reports_merged_and_deleted_chats_as_dirty() {
        let mut book = ChatBook::default();
        book.apply(ChatChange::Message(id("a@lid"), message("x", 1, false)));
        book.apply(ChatChange::Message(
            id("a@s.whatsapp.net"),
            message("y", 2, false),
        ));
        book.take_dirty();

        book.apply(ChatChange::SameChat(id("a@lid"), id("a@s.whatsapp.net")));
        let dirty = book.take_dirty();
        assert!(
            dirty.chats.contains(&id("a@lid")) && dirty.chats.contains(&id("a@s.whatsapp.net"))
        );
        assert_eq!(dirty.aliases, [(id("a@s.whatsapp.net"), id("a@lid"))]);

        book.apply(ChatChange::Deleted(id("a@s.whatsapp.net")));
        assert!(book.take_dirty().chats.contains(&id("a@lid")));
        assert_eq!(book.listed().len(), 0);
    }

    #[test]
    fn gives_everyones_saved_and_chosen_names() {
        let mut book = ChatBook::default();
        let ana = ChatId::new("1@lid");
        book.apply(ChatChange::ContactName(ana.clone(), "Ana Souza".into()));
        book.apply(ChatChange::PushName(ana.clone(), "aninha".into()));
        book.apply(ChatChange::BusinessName(
            ChatId::new("2@lid"),
            "Loja".into(),
            None,
        ));
        let names = book.names();
        assert_eq!(
            names.get(&ana),
            Some(&PersonName {
                saved: Some("Ana Souza".into()),
                chosen: Some("aninha".into()),
            })
        );
        assert_eq!(names.len(), 1);
    }
}
