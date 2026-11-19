//! Who is typing or recording in the open chat, as the conversation shows
//! them, and how the user likes that shown.

use std::path::Path;

use gpui::{App, AppContext as _, Context, SharedString};
use natizap_protocol::ChatId;

use super::MessageLog;
use natizap_protocol::Activity;

/// How the conversation shows who is typing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TypingLook {
    /// Their faces and first names.
    #[default]
    NamesAndFaces,
    /// Their faces alone.
    FacesOnly,
}

impl TypingLook {
    /// How the settings file spells it.
    fn key(self) -> &'static str {
        match self {
            Self::NamesAndFaces => "names",
            Self::FacesOnly => "faces",
        }
    }

    /// The look the settings file at `path` keeps; the default when it
    /// keeps none.
    pub(super) fn read(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| {
                text.lines()
                    .filter_map(|line| line.split_once('='))
                    .find(|(key, _)| key.trim() == "typing")
                    .map(|(_, value)| value.trim().to_owned())
            })
            .and_then(|value| {
                [Self::NamesAndFaces, Self::FacesOnly]
                    .into_iter()
                    .find(|look| look.key() == value)
            })
            .unwrap_or_default()
    }
}

/// Someone typing or recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Typer {
    pub(crate) id: ChatId,
    /// Their first name, or their number.
    pub(crate) name: SharedString,
    pub(crate) activity: Activity,
}

impl MessageLog {
    /// Who is typing or recording in the open chat, this account left out.
    pub(crate) fn typers(&self, cx: &App) -> Vec<Typer> {
        let protocol = self.protocol.read(cx);
        let Some(open) = &self.open else {
            return Vec::new();
        };
        self.typing
            .in_chat(&open.addresses)
            .into_iter()
            .filter(|(who, _)| !protocol.is_own(who))
            .map(|(who, activity)| Typer {
                name: self.first_name(&who),
                id: who,
                activity,
            })
            .collect()
    }

    /// What `person` is called, briefly: the first word of their saved or
    /// chosen name, else their number.
    fn first_name(&self, person: &ChatId) -> SharedString {
        let phone = self
            .group
            .as_ref()
            .and_then(|group| group.phones.iter().find(|(other, _)| other == person))
            .map(|(_, phone)| phone);
        let known = self
            .names
            .get(person)
            .or_else(|| phone.and_then(|phone| self.names.get(phone)));
        known
            .and_then(|known| known.saved.as_deref().or(known.chosen.as_deref()))
            .and_then(|name| name.split_whitespace().next())
            .map(str::to_owned)
            .or_else(|| phone.unwrap_or(person).phone_number())
            .unwrap_or_else(|| "Someone".to_owned())
            .into()
    }

    /// How the conversation shows who is typing.
    #[must_use]
    pub fn typing_look(&self) -> TypingLook {
        self.typing_look
    }

    /// Shows who is typing as `look` from now on, and keeps the choice.
    pub fn set_typing_look(&mut self, look: TypingLook, cx: &mut Context<Self>) {
        self.typing_look = look;
        let path = self.settings_path.clone();
        cx.background_spawn(async move {
            if let Err(error) = std::fs::write(&path, format!("typing={}\n", look.key())) {
                tracing::warn!(%error, "could not keep the chat settings");
            }
        })
        .detach();
        cx.notify();
    }
}
