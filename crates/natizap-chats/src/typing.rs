//! Who is typing or recording in each chat, as its row says it in place of
//! the last message.

use gpui::{Context, Entity, SharedString, Task};
use natizap_protocol::{Activity, ChatId, Protocol, STILL_TYPING};

use crate::list::{ChatList, ChatRow};

impl ChatList {
    /// Follows who types in every chat through `protocol`, and draws again
    /// when someone stops saying so, so their row moves on.
    pub(crate) fn follow_typing(protocol: &Entity<Protocol>, cx: &mut Context<Self>) -> Task<()> {
        let events = protocol
            .read(cx)
            .subscribe(natizap_protocol::Typing::EVENTS);
        cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                let heard = this.update(cx, |list, cx| {
                    if list.typing.hear(&event) {
                        cx.notify();
                    }
                });
                if heard.is_err() {
                    break;
                }
                let this = this.clone();
                cx.spawn(async move |cx| {
                    cx.background_executor().timer(STILL_TYPING).await;
                    let _ = this.update(cx, |_, cx| cx.notify());
                })
                .detach();
            }
        })
    }

    /// What `row` says in place of its last message while someone types or
    /// records in it: "typing…" alone, "Ana is typing…" in a group.
    #[must_use]
    pub fn typing_line(&self, row: &ChatRow) -> Option<SharedString> {
        let typers = self.typing.in_chat(&row.addresses);
        let doing = |activity: Activity| match activity {
            Activity::Typing => "typing…",
            Activity::Recording => "recording audio…",
        };
        let (who, activity) = typers.first()?;
        if !row.id.is_group() {
            return Some(doing(*activity).into());
        }
        Some(match typers.len() {
            1 => format!("{} is {}", self.first_name(who), doing(*activity)).into(),
            many => format!("{many} people are typing…").into(),
        })
    }

    /// The first word of `person`'s saved or chosen name, else their number.
    fn first_name(&self, person: &ChatId) -> String {
        self.names()
            .get(person)
            .and_then(|name| name.saved.as_deref().or(name.chosen.as_deref()))
            .and_then(|name| name.split_whitespace().next())
            .map(str::to_owned)
            .or_else(|| person.phone_number())
            .unwrap_or_else(|| "Someone".to_owned())
    }
}
