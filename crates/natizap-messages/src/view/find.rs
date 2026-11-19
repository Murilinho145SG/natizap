//! Finding text in the open conversation: Ctrl+F or the header's search
//! button opens a bar under the header that searches the whole chat, not
//! only what is loaded, steps through the messages holding the text, and
//! lights it up in them.

use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    Context, Div, Entity, Focusable as _, KeyDownEvent, SharedString, Task, Window, div, px, rgb,
    svg,
};
use natizap_ui::theme::ActiveTheme as _;
use natizap_ui::{Button, ButtonVariant, Icon, Submitted, TextInput};

use super::ConversationView;
use crate::rich_text::Found;
use crate::text_search::Case;

/// How long typing rests before the chat is searched.
const TYPING_REST: Duration = Duration::from_millis(120);
/// The most messages one search steps through.
const MOST_HITS: usize = 1000;

/// The search bar and what it found.
pub(super) struct Find {
    input: Entity<TextInput>,
    case: Case,
    /// The text last searched for.
    query: String,
    /// The messages holding it, newest first, by id and send time.
    hits: Vec<(SharedString, i64)>,
    /// The hit the search is on.
    at: Option<usize>,
    /// Whether the last search came back.
    searched: bool,
    searching: Task<()>,
}

impl std::fmt::Debug for Find {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Find")
            .field("query", &self.query)
            .field("hits", &self.hits.len())
            .field("at", &self.at)
            .finish_non_exhaustive()
    }
}

impl ConversationView {
    /// Opens the search bar, or takes the keyboard back to it.
    pub(super) fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(find) = &self.find {
            window.focus(&find.input.focus_handle(cx), cx);
            return;
        }
        let input = cx.new(|cx| TextInput::new("Search in this chat", cx));
        cx.observe(&input, |this, _, cx| this.search_again(cx))
            .detach();
        cx.subscribe(&input, |this, _, _: &Submitted, cx| {
            this.step_find(true, cx);
        })
        .detach();
        window.focus(&input.focus_handle(cx), cx);
        self.find = Some(Find {
            input,
            case: Case::Insensitive,
            query: String::new(),
            hits: Vec::new(),
            at: None,
            searched: false,
            searching: Task::ready(()),
        });
        cx.notify();
    }

    pub(super) fn close_find(&mut self, cx: &mut Context<Self>) {
        if self.find.take().is_some() {
            self.refocus = true;
            cx.notify();
        }
    }

    /// What the search lights up in the messages on screen.
    pub(super) fn found(&self, cx: &Context<Self>) -> Option<Found> {
        let find = self.find.as_ref()?;
        let query = find.input.read(cx).text().trim();
        (!query.is_empty()).then(|| Found {
            query: query.to_owned().into(),
            case: find.case,
            current: find
                .at
                .and_then(|at| find.hits.get(at))
                .map(|(id, _)| id.clone()),
        })
    }

    /// Searches again once typing rests, if the text changed.
    fn search_again(&mut self, cx: &mut Context<Self>) {
        let Some(find) = self.find.as_mut() else {
            return;
        };
        let query = find.input.read(cx).text().trim().to_owned();
        if query == find.query {
            return;
        }
        find.query.clone_from(&query);
        self.run_find(cx);
    }

    fn run_find(&mut self, cx: &mut Context<Self>) {
        let chats = self
            .log
            .read(cx)
            .open_chat()
            .map(|chat| chat.addresses.clone());
        let Some(find) = self.find.as_mut() else {
            return;
        };
        find.hits.clear();
        find.at = None;
        find.searched = false;
        let (query, case) = (find.query.clone(), find.case);
        if query.is_empty() {
            find.searching = Task::ready(());
            cx.notify();
            return;
        }
        let log = self.log.clone();
        find.searching = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TYPING_REST).await;
            let Ok(searching) = this.update(cx, |_, cx| {
                log.read(cx).search(chats, query, case, MOST_HITS, cx)
            }) else {
                return;
            };
            let found = searching.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(find) = this.find.as_mut() {
                    find.hits = found
                        .into_iter()
                        .map(|message| (message.id.into(), message.sent_at))
                        .collect();
                    find.searched = true;
                    find.at = None;
                }
                this.step_find(true, cx);
            });
        });
        cx.notify();
    }

    /// Goes to the next older message holding the text (`older`), or the
    /// next newer one.
    fn step_find(&mut self, older: bool, cx: &mut Context<Self>) {
        let chats = self
            .log
            .read(cx)
            .open_chat()
            .map_or_else(Vec::new, |chat| chat.addresses.clone());
        let Some(find) = self.find.as_mut() else {
            return;
        };
        let last = find.hits.len().checked_sub(1);
        let at = match (find.at, older) {
            (None, _) => last.map(|_| 0),
            (Some(at), true) => Some(last.map_or(at, |last| (at + 1).min(last))),
            (Some(at), false) => Some(at.saturating_sub(1)),
        };
        find.at = at;
        let hit = at.and_then(|at| find.hits.get(at)).cloned();
        if let Some((id, sent_at)) = hit {
            self.go_to(id, sent_at, chats, cx);
        }
        cx.notify();
    }

    fn toggle_case(&mut self, cx: &mut Context<Self>) {
        if let Some(find) = self.find.as_mut() {
            find.case = match find.case {
                Case::Insensitive => Case::Sensitive,
                Case::Sensitive => Case::Insensitive,
            };
        }
        self.run_find(cx);
    }

    fn find_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        match keystroke.key.as_str() {
            "escape" => self.close_find(cx),
            "enter" if keystroke.modifiers.shift => self.step_find(false, cx),
            "up" => self.step_find(true, cx),
            "down" => self.step_find(false, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    /// The bar under the header, while a search is open.
    pub(super) fn find_bar(&self, cx: &mut Context<Self>) -> Option<Div> {
        let theme = *cx.theme();
        let find = self.find.as_ref()?;
        let count = match (&find.at, find.hits.len(), find.searched) {
            _ if find.query.is_empty() => String::new(),
            (_, _, false) => "Searching…".to_owned(),
            (_, 0, true) => "No results".to_owned(),
            (Some(at), total, true) => format!("{} of {total}", at + 1),
            (None, total, true) => format!("{total} found"),
        };
        Some(
            div()
                .flex_none()
                .h(px(48.0))
                .px_4()
                .flex()
                .items_center()
                .gap_2()
                .bg(rgb(theme.panel))
                .border_b_1()
                .border_color(rgb(theme.line))
                .capture_key_down(cx.listener(|this, event, _, cx| this.find_key(event, cx)))
                .child(
                    svg()
                        .path(Icon::Search.path())
                        .size(px(15.0))
                        .flex_none()
                        .text_color(rgb(theme.muted)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(14.0))
                        .child(find.input.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(px(12.5))
                        .text_color(rgb(theme.muted))
                        .child(count),
                )
                .child(
                    Button::new("find-case")
                        .label("Aa")
                        .variant(ButtonVariant::Ghost)
                        .selected(find.case == Case::Sensitive)
                        .tooltip("Match case")
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_case(cx))),
                )
                .child(
                    Button::new("find-older")
                        .icon(Icon::ChevronUp)
                        .variant(ButtonVariant::Ghost)
                        .round()
                        .tooltip("Older (Enter)")
                        .on_click(cx.listener(|this, _, _, cx| this.step_find(true, cx))),
                )
                .child(
                    Button::new("find-newer")
                        .icon(Icon::ChevronDown)
                        .variant(ButtonVariant::Ghost)
                        .round()
                        .tooltip("Newer (Shift+Enter)")
                        .on_click(cx.listener(|this, _, _, cx| this.step_find(false, cx))),
                )
                .child(
                    Button::new("find-close")
                        .icon(Icon::Close)
                        .variant(ButtonVariant::Ghost)
                        .round()
                        .tooltip("Close (Esc)")
                        .on_click(cx.listener(|this, _, _, cx| this.close_find(cx))),
                ),
        )
    }
}
