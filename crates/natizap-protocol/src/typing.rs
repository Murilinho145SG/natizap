//! Who is typing or recording, in every chat: WhatsApp says so as it
//! happens, and does not say it again to a chat opened afterwards, so
//! whoever shows it keeps track all along.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use whatsapp_rust::prelude::{Event, EventKind};
use whatsapp_rust::types::presence::{ChatPresence, ChatPresenceMedia};

use crate::ChatId;

/// What someone is doing in a chat right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    Typing,
    Recording,
}

/// How long someone counts as typing without saying so again: phones
/// repeat it every few seconds, and a lost "stopped" must not leave them
/// typing for good.
pub const STILL_TYPING: Duration = Duration::from_secs(25);

/// Who is typing or recording in each chat. Feed it every event of
/// [`Typing::EVENTS`].
#[derive(Debug, Clone, Default)]
pub struct Typing(HashMap<ChatId, Vec<(ChatId, Activity, Instant)>>);

impl Typing {
    /// The events typing is followed through.
    pub const EVENTS: &[EventKind] = &[EventKind::ChatPresence];

    /// Takes in what `event` says; `true` when it changed who is typing.
    pub fn hear(&mut self, event: &Event) -> bool {
        let Event::ChatPresence(update) = event else {
            return false;
        };
        let chat = ChatId::new(&update.source.chat.to_non_ad().to_string());
        // In a group the one typing is the sender; alone, the chat.
        let who = ChatId::new(&update.source.sender.to_non_ad().to_string());
        let activity = match (update.state, update.media) {
            (ChatPresence::Composing, ChatPresenceMedia::Audio) => Some(Activity::Recording),
            (ChatPresence::Composing, ChatPresenceMedia::Text) => Some(Activity::Typing),
            (ChatPresence::Paused, _) => None,
        };
        let typers = self.0.entry(chat).or_default();
        typers.retain(|(typer, _, _)| *typer != who);
        if let Some(activity) = activity {
            typers.push((who, activity, Instant::now()));
        }
        true
    }

    /// Who is typing or recording now in the chat at any of `addresses`,
    /// in the order they started.
    #[must_use]
    pub fn in_chat(&self, addresses: &[ChatId]) -> Vec<(ChatId, Activity)> {
        addresses
            .iter()
            .filter_map(|address| self.0.get(address))
            .flatten()
            .filter(|(_, _, since)| since.elapsed() < STILL_TYPING)
            .map(|(who, activity, _)| (who.clone(), *activity))
            .collect()
    }
}
