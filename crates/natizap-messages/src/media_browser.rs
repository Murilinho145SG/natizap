//! Everything a chat shared, in one window: photos and videos, documents
//! and audio files, and links, each in a tab of its own, grouped by month
//! and searchable, as the phone's "Media, links and docs" screen.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{Local, TimeZone as _};
use gpui::prelude::*;
use gpui::{
    Context, Entity, EventEmitter, Focusable as _, ListAlignment, ListState, Pixels, Point,
    SharedString, Task, Window, px,
};
use natizap_ui::TextInput;

use crate::items::MediaView;
use crate::library::{Library, SharedLink, Shelved};
use crate::thumbs;
use natizap_protocol::ChatId;

mod menu;
mod render;

/// Photos and videos in a row of the grid.
const COLUMNS: usize = 6;
/// The user chose a photo, video, document or audio file, and what to do
/// with it.
#[derive(Debug, Clone)]
pub(crate) struct MediaChosen(pub(crate) MediaView, pub(crate) MediaUse);

/// What the user wants done with a chosen attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MediaUse {
    Open,
    SaveAs,
    CopyImage,
}

/// The user asked to go to the message something in the window came in.
#[derive(Debug, Clone)]
pub(crate) struct MessageWanted {
    pub(crate) id: SharedString,
    /// When it was sent, in Unix seconds.
    pub(crate) sent_at: i64,
    /// Every address of its chat.
    pub(crate) chats: Vec<ChatId>,
}

/// The user closed the window.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BrowserClosed;

/// A time the lines rose in.
#[derive(Debug, Clone, Copy)]
struct Entrance {
    /// Which time, naming its animations so each plays once.
    count: u32,
    at: Instant,
}

impl Entrance {
    /// How long lines keep entering after it starts: long enough for the
    /// last line on screen to start late and finish rising.
    const LASTS: Duration = Duration::from_millis(900);
}

/// One of the window's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Media,
    Docs,
    Links,
}

impl Tab {
    const ALL: [Self; 3] = [Self::Media, Self::Docs, Self::Links];

    fn label(self) -> &'static str {
        match self {
            Self::Media => "Media",
            Self::Docs => "Docs",
            Self::Links => "Links",
        }
    }
}

/// Something in the window, by its shelf and index into it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pick {
    Media(usize),
    Doc(usize),
    Link(usize),
}

/// A line of the list: a month's heading, a row of the media grid, a
/// document or a link, by index into the library's shelf.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Line {
    Month(SharedString),
    Tiles(Vec<usize>),
    Doc(usize),
    Link(usize),
}

/// The window showing a chat's media, docs and links.
pub(crate) struct MediaBrowser {
    title: SharedString,
    /// Every address of the chat whose things these are.
    chats: Vec<ChatId>,
    /// Where a right click opened a menu, and on what.
    menu: Option<(Point<Pixels>, Pick)>,
    /// What the chat shared, once read.
    library: Option<Arc<Library>>,
    tab: Tab,
    search: Entity<TextInput>,
    lines: Arc<[Line]>,
    list: ListState,
    /// Whether the search field took the keyboard yet: it does at the
    /// first draw, the first moment a window is at hand.
    focused: bool,
    /// The latest time the lines rose in.
    entrance: Option<Entrance>,
    /// Small copies made while the window shows, by index into the media.
    made: Arc<HashMap<usize, PathBuf>>,
    _reading: Task<()>,
    /// Making the small copies the grid lacks; dropped with the window.
    making: Task<()>,
}

impl std::fmt::Debug for MediaBrowser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaBrowser")
            .field("title", &self.title)
            .field("tab", &self.tab)
            .finish_non_exhaustive()
    }
}

impl EventEmitter<MediaChosen> for MediaBrowser {}
impl EventEmitter<BrowserClosed> for MediaBrowser {}
impl EventEmitter<MessageWanted> for MediaBrowser {}

impl MediaBrowser {
    /// Shows `library` once it is read, under the chat's `title`.
    pub(crate) fn new(
        title: SharedString,
        chats: Vec<ChatId>,
        library: Task<Library>,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| TextInput::new("Search", cx));
        cx.observe(&search, |this, _, cx| this.arrange(cx)).detach();
        let reading = cx.spawn(async move |this, cx| {
            let library = library.await;
            let _ = this.update(cx, |this, cx| {
                this.library = Some(Arc::new(library));
                this.arrange(cx);
                this.enter();
                this.make_small_copies(cx);
            });
        });
        Self {
            title,
            chats,
            menu: None,
            library: None,
            tab: Tab::Media,
            search,
            lines: Arc::from([]),
            list: ListState::new(0, ListAlignment::Top, px(800.0)),
            focused: false,
            entrance: None,
            made: Arc::default(),
            _reading: reading,
            making: Task::ready(()),
        }
    }

    /// Makes the small copies the grid lacks, newest first, showing each
    /// batch as it is done.
    fn make_small_copies(&mut self, cx: &mut Context<Self>) {
        let Some(library) = self.library.clone() else {
            return;
        };
        let wanted = library.small_copies_wanted();
        self.making = cx.spawn(async move |this, cx| {
            thumbs::make_in_batches(wanted, cx, |made, cx| {
                this.update(cx, |this, cx| {
                    Arc::make_mut(&mut this.made).extend(made);
                    cx.notify();
                })
                .is_ok()
            })
            .await;
        });
    }

    /// Has the lines on screen rise in, one after another, as the window
    /// opens or a tab is chosen; typing a search only narrows them.
    fn enter(&mut self) {
        self.entrance = Some(Entrance {
            count: self.entrance.map_or(0, |entrance| entrance.count + 1),
            at: Instant::now(),
        });
    }

    fn choose_tab(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            self.arrange(cx);
            self.enter();
            window.focus(&self.search.focus_handle(cx), cx);
        }
    }

    /// Lays out the chosen tab's items that match the search, by month.
    fn arrange(&mut self, cx: &mut Context<Self>) {
        let Some(library) = self.library.clone() else {
            return;
        };
        let query = self.search.read(cx).text().trim().to_lowercase();
        let matches = |words: &str| query.is_empty() || words.to_lowercase().contains(&query);
        let lines = match self.tab {
            Tab::Media => lay_out(
                &library.media,
                |item| matches(&item.words),
                |items| {
                    items
                        .chunks(COLUMNS)
                        .map(|row| Line::Tiles(row.to_vec()))
                        .collect()
                },
            ),
            Tab::Docs => lay_out(
                &library.docs,
                |item| matches(&item.words),
                |items| items.iter().map(|&index| Line::Doc(index)).collect(),
            ),
            Tab::Links => lay_out(
                &library.links,
                |link| matches(&format!("{} {}", link.shown, link.message)),
                |items| items.iter().map(|&index| Line::Link(index)).collect(),
            ),
        };
        self.lines = lines.into();
        self.list.reset(self.lines.len());
        cx.notify();
    }
}

/// Something that was sent at a time, to group by month.
trait Dated {
    fn sent_at(&self) -> i64;
}

impl Dated for Shelved {
    fn sent_at(&self) -> i64 {
        self.sent_at
    }
}

impl Dated for SharedLink {
    fn sent_at(&self) -> i64 {
        self.sent_at
    }
}

/// The lines for `items` that pass `keep`: each month's heading, then
/// that month's items as `lines_of` lays them out.
fn lay_out<T: Dated>(
    items: &[T],
    keep: impl Fn(&T) -> bool,
    lines_of: impl Fn(&[usize]) -> Vec<Line>,
) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut month: Option<SharedString> = None;
    let mut batch: Vec<usize> = Vec::new();
    for (index, item) in items.iter().enumerate().filter(|(_, item)| keep(item)) {
        let this_month = month_of(item.sent_at());
        if month.as_ref() != Some(&this_month) {
            lines.extend(lines_of(&batch));
            batch.clear();
            lines.push(Line::Month(this_month.clone()));
            month = Some(this_month);
        }
        batch.push(index);
    }
    lines.extend(lines_of(&batch));
    lines
}

/// The month `sent_at` falls in, as a heading: "November 2026".
fn month_of(sent_at: i64) -> SharedString {
    Local.timestamp_opt(sent_at, 0).single().map_or_else(
        || "Earlier".into(),
        |time| time.format("%B %Y").to_string().into(),
    )
}

#[cfg(test)]
mod tests {
    use chrono::{Local, TimeZone as _};

    use super::{Dated, Line, lay_out};

    struct Sent(i64);

    impl Dated for Sent {
        fn sent_at(&self) -> i64 {
            self.0
        }
    }

    fn at(year: i32, month: u32, day: u32) -> Sent {
        Sent(
            Local
                .with_ymd_and_hms(year, month, day, 12, 0, 0)
                .single()
                .map_or(0, |time| time.timestamp()),
        )
    }

    fn one_per_line(items: &[usize]) -> Vec<Line> {
        items.iter().map(|&index| Line::Doc(index)).collect()
    }

    #[test]
    fn heads_each_month_once() {
        let items = [at(2026, 11, 20), at(2026, 11, 3), at(2026, 10, 30)];
        assert_eq!(
            lay_out(&items, |_| true, one_per_line),
            vec![
                Line::Month("November 2026".into()),
                Line::Doc(0),
                Line::Doc(1),
                Line::Month("October 2026".into()),
                Line::Doc(2),
            ]
        );
    }

    #[test]
    fn leaves_out_months_without_matches() {
        let items = [at(2026, 11, 20), at(2026, 10, 30)];
        assert_eq!(
            lay_out(&items, |item| item.0 == items[1].0, one_per_line),
            vec![Line::Month("October 2026".into()), Line::Doc(1)]
        );
    }

    #[test]
    fn fills_grid_rows_within_a_month() {
        let items: Vec<Sent> = (1..=8).map(|day| at(2026, 11, day)).collect();
        let rows = lay_out(
            &items,
            |_| true,
            |items| {
                items
                    .chunks(6)
                    .map(|row| Line::Tiles(row.to_vec()))
                    .collect()
            },
        );
        assert_eq!(
            rows,
            vec![
                Line::Month("November 2026".into()),
                Line::Tiles(vec![0, 1, 2, 3, 4, 5]),
                Line::Tiles(vec![6, 7]),
            ]
        );
    }
}
