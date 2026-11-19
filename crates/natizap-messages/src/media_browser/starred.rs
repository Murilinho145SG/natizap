//! The Starred tab's rows: each starred message, who wrote it and when;
//! a click goes to it in the chat.

use gpui::prelude::*;
use gpui::{AnyElement, SharedString, WeakEntity};
use natizap_ui::Icon;
use natizap_ui::theme::Theme;

use super::render::{badge, day, entry};
use super::{MediaBrowser, MessageWanted};
use crate::library::Starred;

/// How big the star beside each message is.
const STAR: f32 = 40.0;

pub(super) fn row(
    starred: &Starred,
    browser: &WeakEntity<MediaBrowser>,
    theme: Theme,
) -> AnyElement {
    let who = starred
        .author
        .clone()
        .filter(|author| !author.is_empty())
        .unwrap_or_else(|| "You".into());
    let below = vec![SharedString::from(format!(
        "{who} · {}",
        day(starred.sent_at)
    ))];
    let (id, sent_at, browser) = (starred.message_id.clone(), starred.sent_at, browser.clone());
    entry(
        SharedString::from(format!("starred-{}", starred.message_id)),
        badge(Icon::Star, STAR, theme),
        starred.summary.clone(),
        theme.text,
        below,
        theme,
    )
    .on_click(move |_, _, cx| {
        let id = id.clone();
        let _ = browser.update(cx, |browser, cx| {
            cx.emit(MessageWanted {
                id,
                sent_at,
                chats: browser.chats.clone(),
            });
        });
    })
    .into_any_element()
}
