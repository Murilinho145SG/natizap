//! Who wrote each group message: their name, number, color and role.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::SharedString;
use natizap_protocol::{ChatId, PersonName};
use natizap_ui::name_color;

use crate::message::Message;

/// Who wrote a group message, as its bubble names them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Author {
    /// Their address, for their picture.
    pub(crate) id: ChatId,
    /// The name they chose, marked with `~` as the phone does for people not
    /// in the address book, or else their number.
    pub(crate) name: SharedString,
    /// Their number beside a chosen name, when known.
    pub(crate) phone: Option<SharedString>,
    pub(crate) color: u32,
    /// Their role under their name, e.g. "Community admin".
    pub(crate) role: Option<SharedString>,
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
        let (name, phone) = match (saved, chosen) {
            (Some(saved), _) => (saved.to_owned(), None),
            (None, Some(chosen)) => (format!("~ {chosen}"), phone),
            (None, None) => (phone.unwrap_or_else(|| sender.as_str().to_owned()), None),
        };
        Self {
            id: sender.clone(),
            name: name.into(),
            phone: phone.map(Into::into),
            color: name_color(sender.as_str()),
            role: senders
                .members
                .community_admins
                .iter()
                .any(|admin| admin == sender || admin == phone_address)
                .then(|| "Community admin".into()),
        }
    }
}
