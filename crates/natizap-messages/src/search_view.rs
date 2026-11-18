//! The messages of every chat holding what is typed in the chat list's
//! search field, newest first, each a way to the message.

use std::collections::HashMap;
use std::ops::Range;
use std::time::Duration;

use chrono::{Local, TimeZone as _};
use gpui::prelude::*;
use gpui::{
    Context, Entity, EventEmitter, FontWeight, HighlightStyle, SharedString, StyledText, Task,
    UniformListScrollHandle, Window, div, px, rgb, uniform_list,
};
use natizap_protocol::{ChatId, Marked};
use natizap_ui::theme::{ACCENT, HOVER, MUTED, TEXT};
use natizap_ui::{Button, ButtonVariant, scrollbar};

use crate::log::MessageLog;
use crate::message::Message;
use crate::text_search::{Case, occurrences};

/// How long typing rests before every chat is searched.
const TYPING_REST: Duration = Duration::from_millis(150);
/// The most messages shown.
const MOST_HITS: usize = 200;
const ROW_HEIGHT: f32 = 64.0;
/// Characters kept before the match in a hit's line, so it shows.
const LEAD: usize = 24;

/// The user picked a message among the hits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHitOpened {
    /// The chat it is in.
    pub chat: ChatId,
    /// The message.
    pub id: SharedString,
    /// When it was sent, to show the chat back far enough.
    pub sent_at: i64,
}

/// The messages holding a query, in every chat.
pub struct MessageSearch {
    log: Entity<MessageLog>,
    titles: HashMap<ChatId, SharedString>,
    query: String,
    case: Case,
    hits: Vec<Message>,
    searched: bool,
    searching: Task<()>,
    scroll: UniformListScrollHandle,
}

impl std::fmt::Debug for MessageSearch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MessageSearch")
            .field("query", &self.query)
            .field("hits", &self.hits.len())
            .finish_non_exhaustive()
    }
}

impl EventEmitter<SearchHitOpened> for MessageSearch {}

impl MessageSearch {
    /// Searches the messages `log` keeps.
    #[must_use]
    pub fn new(log: Entity<MessageLog>) -> Self {
        Self {
            log,
            titles: HashMap::new(),
            query: String::new(),
            case: Case::Insensitive,
            hits: Vec::new(),
            searched: false,
            searching: Task::ready(()),
            scroll: UniformListScrollHandle::new(),
        }
    }

    /// Names the chats the hits are in, by every address they go by.
    pub fn set_titles(&mut self, titles: HashMap<ChatId, SharedString>, cx: &mut Context<Self>) {
        self.titles = titles;
        cx.notify();
    }

    /// Searches every chat for `query` once typing rests; nothing for an
    /// empty one.
    pub fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        let query = query.trim();
        if query == self.query {
            return;
        }
        query.clone_into(&mut self.query);
        self.run(cx);
    }

    fn run(&mut self, cx: &mut Context<Self>) {
        self.searched = false;
        if self.query.is_empty() {
            self.hits.clear();
            self.searching = Task::ready(());
            cx.notify();
            return;
        }
        let (query, case, log) = (self.query.clone(), self.case, self.log.clone());
        self.searching = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TYPING_REST).await;
            let Ok(searching) = this.update(cx, |_, cx| {
                log.read(cx).search(None, query, case, MOST_HITS, cx)
            }) else {
                return;
            };
            let found = searching.await;
            let _ = this.update(cx, |this, cx| {
                this.hits = found;
                this.searched = true;
                this.scroll.scroll_to_item(0, gpui::ScrollStrategy::Top);
                cx.notify();
            });
        });
        cx.notify();
    }

    fn toggle_case(&mut self, cx: &mut Context<Self>) {
        self.case = match self.case {
            Case::Insensitive => Case::Sensitive,
            Case::Sensitive => Case::Insensitive,
        };
        self.run(cx);
    }

    fn title(&self, chat: &ChatId) -> SharedString {
        self.titles.get(chat).cloned().unwrap_or_else(|| {
            chat.phone_number()
                .unwrap_or_else(|| chat.as_str().to_owned())
                .into()
        })
    }
}

impl Render for MessageSearch {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = match (self.searched, self.hits.len()) {
            (false, _) => "Searching…".to_owned(),
            (true, 0) => "No messages".to_owned(),
            (true, count) if count >= MOST_HITS => format!("Newest {count}"),
            (true, count) => format!("{count}"),
        };
        let rows: Vec<HitRow> = self
            .hits
            .iter()
            .map(|message| HitRow::of(message, &self.title(&message.chat), &self.query, self.case))
            .collect();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .px_5()
                    .pt_2()
                    .pb_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(13.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(ACCENT))
                            .child("Messages"),
                    )
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(rgb(MUTED))
                            .child(status),
                    )
                    .child(
                        Button::new("search-case")
                            .label("Aa")
                            .variant(ButtonVariant::Ghost)
                            .selected(self.case == Case::Sensitive)
                            .tooltip("Match case")
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_case(cx))),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(
                        uniform_list(
                            "message-hits",
                            rows.len(),
                            cx.processor(move |_, range: Range<usize>, _, cx| {
                                rows[range]
                                    .iter()
                                    .map(|row| row.render(cx))
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .track_scroll(&self.scroll)
                        .flex_1()
                        .pb_2(),
                    )
                    .child(scrollbar(
                        "message-hits-scrollbar",
                        &self.scroll,
                        window,
                        cx,
                    )),
            )
    }
}

/// One hit as its row shows it.
#[derive(Clone)]
struct HitRow {
    opened: SearchHitOpened,
    title: SharedString,
    when: SharedString,
    /// Who wrote it, in groups, or "You".
    by: Option<SharedString>,
    /// The line around the match, and where the match is in it.
    line: SharedString,
    found: Vec<Range<usize>>,
}

impl HitRow {
    fn of(message: &Message, title: &SharedString, query: &str, case: Case) -> Self {
        let text = Marked::parse(message.text.as_deref().unwrap_or_default()).text;
        let flat = text.replace('\n', " ");
        // Starts a little before the first match, so the match shows.
        let first = occurrences(&flat, query, case)
            .first()
            .map_or(0, |range| range.start);
        let start = flat[..first]
            .char_indices()
            .rev()
            .nth(LEAD)
            .map_or(0, |(index, _)| index);
        let line = if start > 0 {
            format!("…{}", &flat[start..])
        } else {
            flat
        };
        let found = occurrences(&line, query, case);
        let by = if message.from_me {
            Some("You".into())
        } else {
            message.author.clone().map(Into::into)
        };
        Self {
            opened: SearchHitOpened {
                chat: message.chat.clone(),
                id: message.id.clone().into(),
                sent_at: message.sent_at,
            },
            title: title.clone(),
            when: when(message.sent_at).into(),
            by,
            line: line.into(),
            found,
        }
    }

    fn render(&self, cx: &mut Context<MessageSearch>) -> impl IntoElement + use<> {
        let opened = self.opened.clone();
        let highlights = self.found.iter().map(|range| {
            (
                range.clone(),
                HighlightStyle {
                    color: Some(rgb(TEXT).into()),
                    font_weight: Some(FontWeight::BOLD),
                    ..HighlightStyle::default()
                },
            )
        });
        let line =
            StyledText::new(self.line.clone()).with_highlights(highlights.collect::<Vec<_>>());
        div().w_full().px_2().child(
            div()
                .id(SharedString::from(format!("hit-{}", self.opened.id)))
                .h(px(ROW_HEIGHT))
                .px_3()
                .flex()
                .flex_col()
                .justify_center()
                .gap_0p5()
                .rounded_lg()
                .cursor_pointer()
                .hover(|row| row.bg(gpui::rgba(HOVER)))
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(opened.clone())))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .font_weight(FontWeight::MEDIUM)
                                .child(self.title.clone()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(12.0))
                                .text_color(rgb(MUTED))
                                .child(self.when.clone()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .min_w_0()
                        .text_size(px(13.5))
                        .text_color(rgb(MUTED))
                        .children(
                            self.by
                                .clone()
                                .map(|by| div().flex_none().child(format!("{by}: "))),
                        )
                        .child(div().min_w_0().truncate().child(line)),
                ),
        )
    }
}

/// When a hit was sent: the time today, else the date.
fn when(sent_at: i64) -> String {
    let Some(sent) = Local.timestamp_opt(sent_at, 0).single() else {
        return String::new();
    };
    if sent.date_naive() == Local::now().date_naive() {
        sent.format("%H:%M").to_string()
    } else {
        sent.format("%d/%m/%Y").to_string()
    }
}
