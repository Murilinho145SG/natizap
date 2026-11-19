//! Who wrote each group message: their name, number, color and role.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::SharedString;
use natizap_protocol::{ChatId, PersonName};
use natizap_ui::name_tint;
use natizap_ui::theme::Theme;

use crate::message::Message;

/// Who wrote a group message, as its bubble names them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Author {
    /// Their address, for their picture.
    pub(crate) id: ChatId,
    /// Where their profile is asked for: their phone-number address when
    /// known, as a LID tells less.
    pub(crate) profile: ChatId,
    /// The name they chose, marked with `~` as the phone does for people not
    /// in the address book, or else their number.
    pub(crate) name: SharedString,
    /// Their number beside a chosen name, when known.
    pub(crate) phone: Option<SharedString>,
    pub(crate) color: Tint,
    /// Their role under their name, e.g. "Community admin".
    pub(crate) role: Option<SharedString>,
}

/// The colour a name is drawn in, chosen while the theme is not at hand
/// and read from it when drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tint {
    /// The accent, for this account.
    Accent,
    /// Secondary text, for someone unknown.
    Muted,
    /// One of the theme's name tints, by place.
    Name(usize),
}

impl Tint {
    /// The colour `self` is in `theme`.
    pub(crate) fn of(self, theme: Theme) -> u32 {
        match self {
            Self::Accent => theme.accent,
            Self::Muted => theme.muted,
            Self::Name(place) => theme.name_tints[place % theme.name_tints.len()],
        }
    }

    /// The tint of the person at `seed`.
    pub(crate) fn person(seed: &str) -> Self {
        Self::Name(name_tint(seed))
    }
}

/// What the group says about its members, beyond each message.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Members {
    /// When the chat is a community's announcements group, its admins.
    pub(crate) community_admins: Vec<ChatId>,
    /// Each member's phone-number address by their other address.
    pub(crate) phones: HashMap<ChatId, ChatId>,
    /// What people are called, by address, for messages that came without
    /// a name and for names saved in the address book.
    pub(crate) names: Arc<HashMap<ChatId, PersonName>>,
}

/// Everything known about who wrote the messages laid out together.
pub(crate) struct Senders<'a> {
    members: &'a Members,
    /// The name each sender chose, from whichever of their messages carries
    /// one: older messages may have come without it.
    names: HashMap<&'a ChatId, &'a str>,
}

impl<'a> Senders<'a> {
    pub(crate) fn of(messages: &'a [Message], members: &'a Members) -> Self {
        let names = messages
            .iter()
            .filter_map(|message| Some((message.sender.as_ref()?, message.author.as_deref()?)))
            .collect();
        Self { members, names }
    }
}

impl Senders<'_> {
    /// What `person` is called, as their messages would name them.
    pub(crate) fn name_of(&self, person: &ChatId) -> SharedString {
        let phone_address = self.members.phones.get(person).unwrap_or(person);
        let known = self
            .members
            .names
            .get(person)
            .or_else(|| self.members.names.get(phone_address));
        let saved = known.and_then(|known| known.saved.as_deref());
        let chosen = self
            .names
            .get(person)
            .copied()
            .or_else(|| known.and_then(|known| known.chosen.as_deref()));
        shown_name(saved, chosen, phone_address.phone_number(), person)
            .0
            .into()
    }
}

impl Senders<'_> {
    /// Who a mention's `digits` name, as the mention shows them: the
    /// address book's name, else the one they chose, else their number.
    /// `None` for a LID nobody here knows.
    pub(crate) fn mentioned(&self, digits: &str) -> Option<String> {
        ["lid", "s.whatsapp.net"].into_iter().find_map(|server| {
            let person = ChatId::new(&format!("{digits}@{server}"));
            let phone = self.members.phones.get(&person).unwrap_or(&person);
            let known = [&person, phone]
                .into_iter()
                .find_map(|address| self.members.names.get(address));
            known
                .and_then(|known| known.saved.clone())
                .or_else(|| self.names.get(&person).map(|&chosen| chosen.to_owned()))
                .or_else(|| known.and_then(|known| known.chosen.clone()))
                // Only a number its country's plan knows comes back spaced
                // out: unknown digits may be a LID, and stay digits.
                .or_else(|| phone.phone_number().filter(|shown| shown.contains(' ')))
        })
    }
}

/// As on the phone: the address book's name, else the one the person
/// chose, marked with `~` and shown with their `phone`, else the number.
fn shown_name(
    saved: Option<&str>,
    chosen: Option<&str>,
    phone: Option<String>,
    id: &ChatId,
) -> (String, Option<String>) {
    match (saved, chosen) {
        (Some(saved), _) => (saved.to_owned(), None),
        (None, Some(chosen)) => (format!("~ {chosen}"), phone),
        (None, None) => (phone.unwrap_or_else(|| id.as_str().to_owned()), None),
    }
}

impl Author {
    /// The author of `message`, sent by `sender` in a group.
    pub(crate) fn of(message: &Message, sender: &ChatId, senders: &Senders<'_>) -> Self {
        let phone_address = message
            .sender_phone
            .as_ref()
            .or_else(|| senders.members.phones.get(sender))
            .unwrap_or(sender);
        let phone = phone_address.phone_number();
        let known = senders
            .members
            .names
            .get(sender)
            .or_else(|| senders.members.names.get(phone_address));
        // As on the phone: the address book's name, else the one the
        // person chose, marked with `~` and shown with their number.
        let saved = known.and_then(|known| known.saved.as_deref());
        let chosen = message
            .author
            .as_deref()
            .or_else(|| senders.names.get(sender).copied())
            .or_else(|| known.and_then(|known| known.chosen.as_deref()));
        let (name, phone) = shown_name(saved, chosen, phone, sender);
        Self {
            id: sender.clone(),
            profile: phone_address.clone(),
            name: name.into(),
            phone: phone.map(Into::into),
            color: Tint::person(sender.as_str()),
            role: senders
                .members
                .community_admins
                .iter()
                .any(|admin| admin == sender || admin == phone_address)
                .then(|| "Community admin".into()),
        }
    }
}
