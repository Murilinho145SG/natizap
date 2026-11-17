use std::collections::{HashMap, HashSet};

use natizap_protocol::ChatId;

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
    /// Name saved in the phone's address book.
    pub(crate) contact_name: Option<String>,
    /// Group subject, or the name WhatsApp attached to the chat.
    pub(crate) subject: Option<String>,
    /// Name the other person chose for themselves.
    pub(crate) push_name: Option<String>,
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
}

impl Chat {
    fn new(id: ChatId) -> Self {
        Self {
            id,
            contact_name: None,
            subject: None,
            push_name: None,
            preview: None,
            active_at: 0,
            unread: 0,
            marked_unread: false,
            pinned_at: None,
            mute: None,
            archived: false,
        }
    }

    /// The name to show: the address book wins, as on the phone, then the
    /// chat's own name, the person's self-chosen name, and the phone number.
    #[must_use]
    pub(crate) fn title(&self) -> String {
        self.contact_name
            .clone()
            .or_else(|| self.subject.clone())
            .or_else(|| self.push_name.as_ref().map(|name| format!("~ {name}")))
            .or_else(|| self.id.phone_number())
            .unwrap_or_else(|| self.id.as_str().to_owned())
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
        self.contact_name = self.contact_name.take().or(other.contact_name);
        self.subject = self.subject.take().or(other.subject);
        self.push_name = self.push_name.take().or(other.push_name);
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
    /// Pinned at the given time, or unpinned.
    Pinned(ChatId, Option<i64>),
    /// Muted, or unmuted.
    Muted(ChatId, Option<Mute>),
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
}

/// Every known chat, and the rules for folding changes into them.
#[derive(Debug, Default)]
pub(crate) struct ChatBook {
    chats: HashMap<ChatId, Chat>,
    aliases: HashMap<ChatId, ChatId>,
    dirty: Dirty,
}

impl ChatBook {
    /// Rebuilds a book from persisted chats and aliases.
    pub(crate) fn restore(
        chats: impl IntoIterator<Item = Chat>,
        aliases: impl IntoIterator<Item = (ChatId, ChatId)>,
    ) -> Self {
        Self {
            chats: chats
                .into_iter()
                .map(|chat| (chat.id.clone(), chat))
                .collect(),
            aliases: aliases.into_iter().collect(),
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
            }),
            ChatChange::Message(id, preview) => self.update(&id, |chat| {
                if preview.from_me {
                    // Replying from any device reads the chat, as on mobile.
                    chat.unread = 0;
                    chat.marked_unread = false;
                } else {
                    chat.unread += 1;
                }
                chat.show(preview);
            }),
            ChatChange::ContactName(id, name) => self.update(&id, |chat| {
                chat.contact_name = Some(name);
            }),
            ChatChange::PushName(id, name) => self.update(&id, |chat| chat.push_name = Some(name)),
            ChatChange::Pinned(id, at) => self.update(&id, |chat| chat.pinned_at = at),
            ChatChange::Muted(id, mute) => self.update(&id, |chat| chat.mute = mute),
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
        let mut chats: Vec<&Chat> = self.chats.values().filter(|chat| !chat.archived).collect();
        chats.sort_by(|a, b| {
            b.pinned_at
                .cmp(&a.pinned_at)
                .then(b.active_at.cmp(&a.active_at))
                .then_with(|| a.id.cmp(&b.id))
        });
        chats
    }

    /// How many chats are archived.
    #[must_use]
    pub(crate) fn archived_count(&self) -> usize {
        self.chats.values().filter(|chat| chat.archived).count()
    }

    /// Returns and forgets what changed since the previous call.
    pub(crate) fn take_dirty(&mut self) -> Dirty {
        std::mem::take(&mut self.dirty)
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
        assert_eq!(book.archived_count(), 1);
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

    #[test]
    fn title_prefers_the_address_book_then_falls_back_to_the_number() {
        let mut book = ChatBook::default();
        let chat = id("5511999@s.whatsapp.net");
        book.apply(ChatChange::Message(chat.clone(), message("hi", 1, false)));
        assert_eq!(
            book.get(&chat).map(Chat::title).as_deref(),
            Some("+5511999")
        );

        book.apply(ChatChange::PushName(chat.clone(), "Ana".into()));
        assert_eq!(book.get(&chat).map(Chat::title).as_deref(), Some("~ Ana"));

        book.apply(ChatChange::ContactName(chat.clone(), "Ana Souza".into()));
        assert_eq!(
            book.get(&chat).map(Chat::title).as_deref(),
            Some("Ana Souza")
        );
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
}
