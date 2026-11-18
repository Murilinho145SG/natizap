//! How the media, docs and links window is drawn.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::{Local, TimeZone as _};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Div, Focusable as _, FontWeight, ImageSource, ObjectFit,
    SharedString, Stateful, WeakEntity, Window, div, img, list, px, rgb, rgba, svg,
};
use natizap_protocol::ContentKind;
use natizap_ui::motion::rise_in_after;
use natizap_ui::theme::{
    ACCENT, ACCENT_INK, DEEP, HOVER, LINE, LINK, MUTED, PANEL, RAISED, SCRIM, TEXT, glass,
};
use natizap_ui::{Button, ButtonVariant, Icon, dialog};

use super::menu::{RIGHT, on_right_click};
use super::{BrowserClosed, COLUMNS, Entrance, Line, MediaBrowser, MediaChosen, Pick, Tab};
use crate::items::MediaView;
use crate::library::{Library, SharedLink, Shelved};

/// Lines that rise in one after another; the rest are off screen.
const ENTERING_LINES: usize = 16;
/// How far each line rises, in pixels.
const LINE_RISE: f32 = 10.0;
/// How much later each line starts rising than the one above.
const LINE_STAGGER: Duration = Duration::from_millis(28);

/// Side of a photo or video in the grid.
const TILE: f32 = 128.0;
/// Space between tiles.
const GAP: f32 = 6.0;
/// Padding inside the window.
const INSET: f32 = 20.0;

impl Render for MediaBrowser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !std::mem::replace(&mut self.focused, true) {
            window.focus(&self.search.focus_handle(cx), cx);
        }
        let viewport = window.viewport_size();
        #[expect(clippy::cast_precision_loss, reason = "a handful of columns")]
        let grid = COLUMNS as f32 * TILE + (COLUMNS - 1) as f32 * GAP;
        let width = (grid + 2.0 * INSET).min(f32::from(viewport.width) * 0.94);
        let height = 720.0_f32.min(f32::from(viewport.height) * 0.88);
        let panel = div()
            .id("media-browser")
            .w(px(width))
            .h(px(height))
            .flex()
            .flex_col()
            .rounded_xl()
            .bg(rgb(PANEL))
            .map(glass)
            .occlude()
            .text_color(rgb(TEXT))
            .child(self.header(cx))
            .child(self.toolbar(cx))
            .child(self.body(cx));
        let panel = panel.children(self.menu(cx));
        let browser = cx.weak_entity();
        dialog(window, panel, move |_, cx| {
            let _ = browser.update(cx, |_, cx| cx.emit(BrowserClosed));
        })
    }
}

impl MediaBrowser {
    fn header(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .items_center()
            .gap_3()
            .px(px(INSET))
            .pt_4()
            .pb_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(16.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Media, links and docs"),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(12.5))
                            .text_color(rgb(MUTED))
                            .child(self.title.clone()),
                    ),
            )
            .child(
                Button::new("close-media-browser")
                    .icon(Icon::Close)
                    .variant(ButtonVariant::Ghost)
                    .round()
                    .tooltip("Close")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(BrowserClosed))),
            )
    }

    /// The tabs, each with how many it holds, and the search field.
    fn toolbar(&self, cx: &mut Context<Self>) -> Div {
        let counts = self
            .library
            .as_ref()
            .map(|library| [library.media.len(), library.docs.len(), library.links.len()]);
        div()
            .flex()
            .items_center()
            .gap_1p5()
            .px(px(INSET))
            .pb_3()
            .border_b_1()
            .border_color(rgb(LINE))
            .children(Tab::ALL.into_iter().enumerate().map(|(index, tab)| {
                let on = tab == self.tab;
                let label: SharedString = match counts {
                    Some(counts) => format!("{} {}", tab.label(), counts[index]).into(),
                    None => tab.label().into(),
                };
                div()
                    .id(tab.label())
                    .px_3()
                    .h(px(28.0))
                    .flex()
                    .items_center()
                    .rounded_full()
                    .border_1()
                    .text_size(px(13.0))
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .map(|chip| {
                        if on {
                            chip.bg(rgb(ACCENT))
                                .border_color(rgb(ACCENT))
                                .text_color(rgb(ACCENT_INK))
                        } else {
                            chip.border_color(rgb(LINE))
                                .text_color(rgb(MUTED))
                                .hover(|chip| chip.text_color(rgb(TEXT)))
                        }
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.choose_tab(tab, window, cx);
                    }))
                    .child(label)
            }))
            .child(div().flex_1())
            .child(
                div()
                    .w(px(240.0))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_1p5()
                    .rounded_full()
                    .bg(rgb(RAISED))
                    .text_size(px(13.5))
                    .child(
                        svg()
                            .path(Icon::Search.path())
                            .size(px(14.0))
                            .flex_none()
                            .text_color(rgb(MUTED)),
                    )
                    .child(div().flex_1().min_w_0().child(self.search.clone())),
            )
    }

    /// The chosen tab's items, or why there are none.
    fn body(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(library) = self.library.clone() else {
            return notice("Reading the chat…");
        };
        if self.lines.is_empty() {
            let searching = !self.search.read(cx).text().trim().is_empty();
            return notice(match (searching, self.tab) {
                (true, _) => "Nothing matches the search",
                (false, Tab::Media) => "No photos or videos yet",
                (false, Tab::Docs) => "No documents yet",
                (false, Tab::Links) => "No links yet",
            });
        }
        let (lines, made) = (Arc::clone(&self.lines), Arc::clone(&self.made));
        let browser = cx.weak_entity();
        let entering = self
            .entrance
            .filter(|entrance| entrance.at.elapsed() < Entrance::LASTS)
            .map(|entrance| entrance.count);
        list(self.list.clone(), move |index, _, _| {
            let Some(line) = lines.get(index) else {
                return div().into_any_element();
            };
            let shown = render_line(line, &library, &made, &browser);
            match entering {
                Some(count) if index < ENTERING_LINES => rise_in_after(
                    SharedString::from(format!("line-{count}-{index}")),
                    div().w_full().child(shown),
                    LINE_RISE,
                    LINE_STAGGER * u32::try_from(index).unwrap_or(0),
                )
                .into_any_element(),
                _ => shown,
            }
        })
        .flex_1()
        .min_h_0()
        .px(px(INSET))
        .pb_4()
        .into_any_element()
    }
}

fn notice(text: &'static str) -> AnyElement {
    div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(14.0))
        .text_color(rgb(MUTED))
        .child(text)
        .into_any_element()
}

fn render_line(
    line: &Line,
    library: &Library,
    made: &HashMap<usize, PathBuf>,
    browser: &WeakEntity<MediaBrowser>,
) -> AnyElement {
    match line {
        Line::Month(month) => div()
            .pt_4()
            .pb_2()
            .text_size(px(12.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(MUTED))
            .child(month.clone())
            .into_any_element(),
        Line::Tiles(indices) => div()
            .flex()
            .gap(px(GAP))
            .pb(px(GAP))
            .children(indices.iter().filter_map(|&index| {
                let item = library.media.get(index)?;
                let small = item.small.as_ref().or_else(|| made.get(&index));
                Some(tile(item, small, browser, Pick::Media(index)))
            }))
            .into_any_element(),
        Line::Doc(index) => library.docs.get(*index).map_or_else(
            || div().into_any_element(),
            |item| doc(item, browser, Pick::Doc(*index)),
        ),
        Line::Link(index) => library.links.get(*index).map_or_else(
            || div().into_any_element(),
            |shown| link(shown, browser, Pick::Link(*index)),
        ),
    }
}

/// What a photo or video shows in the grid: its small copy, else the
/// preview that came with the message until the copy is made. Never the
/// original, which would be decoded at full size for a tile.
fn picture(media: &MediaView, small: Option<&PathBuf>) -> Option<ImageSource> {
    small
        .cloned()
        .map(Into::into)
        .or_else(|| media.preview.clone().map(Into::into))
}

fn choose(browser: &WeakEntity<MediaBrowser>, media: &MediaView, cx: &mut App) {
    let media = media.clone();
    let _ = browser.update(cx, |_, cx| cx.emit(MediaChosen(media)));
}

fn tile(
    item: &Shelved,
    small: Option<&PathBuf>,
    browser: &WeakEntity<MediaBrowser>,
    pick: Pick,
) -> AnyElement {
    let right_click = on_right_click(browser, pick);
    let media = item.media.clone();
    let browser = browser.clone();
    let video = media.fetch.kind == ContentKind::Video;
    div()
        .id(SharedString::from(media.fetch.message_id.clone()))
        .relative()
        .size(px(TILE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .rounded_lg()
        .bg(rgb(DEEP))
        .cursor_pointer()
        .hover(|tile| tile.opacity(0.85))
        .active(|tile| tile.opacity(0.7))
        .child(match picture(&media, small) {
            Some(picture) => img(picture)
                .size(px(TILE))
                .flex_none()
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None => svg()
                .path(if video { Icon::Video } else { Icon::Camera }.path())
                .size(px(24.0))
                .text_color(rgb(MUTED))
                .into_any_element(),
        })
        .when(video, |tile| {
            tile.child(
                div()
                    .absolute()
                    .bottom_1()
                    .left_1()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_1()
                    .rounded_md()
                    .bg(rgba(SCRIM))
                    .text_size(px(11.0))
                    .text_color(rgb(TEXT))
                    .child(
                        svg()
                            .path(Icon::Video.path())
                            .size(px(11.0))
                            .text_color(rgb(TEXT)),
                    )
                    .children(media.detail.clone()),
            )
        })
        .on_mouse_down(RIGHT, right_click)
        .on_click(move |_, _, cx| choose(&browser, &media, cx))
        .into_any_element()
}

/// A row: something to know it by on the left, a title, and muted lines
/// below. It lights up on hover and dims while pressed.
fn entry(
    id: SharedString,
    leading: AnyElement,
    title: SharedString,
    title_color: u32,
    below: Vec<SharedString>,
) -> Stateful<Div> {
    div()
        .id(id)
        // The full width: the hover fills the row, and long titles are cut
        // at the window's edge instead of running past it.
        .w_full()
        .min_w_0()
        .flex()
        .items_center()
        .gap_3()
        .px_2()
        .py_2()
        .rounded_lg()
        .cursor_pointer()
        .hover(|row| row.bg(rgba(HOVER)))
        .active(|row| row.bg(rgb(RAISED)))
        .child(leading)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .truncate()
                        .text_size(px(14.0))
                        .text_color(rgb(title_color))
                        .child(title),
                )
                .children(below.into_iter().map(|line| {
                    div()
                        .truncate()
                        .text_size(px(12.5))
                        .text_color(rgb(MUTED))
                        .child(line)
                })),
        )
}

/// A rounded square holding `icon`, `side` pixels wide.
fn badge(icon: Icon, side: f32) -> AnyElement {
    div()
        .size(px(side))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_lg()
        .bg(rgb(RAISED))
        .child(
            svg()
                .path(icon.path())
                .size(px(18.0))
                .text_color(rgb(MUTED)),
        )
        .into_any_element()
}

fn doc(item: &Shelved, browser: &WeakEntity<MediaBrowser>, pick: Pick) -> AnyElement {
    let right_click = on_right_click(browser, pick);
    let media = item.media.clone();
    let browser = browser.clone();
    let audio = media.fetch.kind == ContentKind::Audio;
    let title = media
        .name
        .clone()
        .unwrap_or_else(|| (if audio { "Audio" } else { "Document" }).into());
    let below = [
        media.detail.as_ref().map(ToString::to_string),
        Some(day(item.sent_at)),
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" · ");
    entry(
        SharedString::from(media.fetch.message_id.clone()),
        badge(if audio { Icon::Waveform } else { Icon::File }, 40.0),
        title,
        TEXT,
        vec![below.into()],
    )
    .on_mouse_down(RIGHT, right_click)
    .on_click(move |_, _, cx| choose(&browser, &media, cx))
    .into_any_element()
}

/// Side of a link's picture.
const LINK_PICTURE: f32 = 56.0;

/// A link as the phone lists it: the page's picture and title, what it
/// says about itself, and where it leads; a link without a preview shows
/// itself and the message it came in.
fn link(link: &SharedLink, browser: &WeakEntity<MediaBrowser>, pick: Pick) -> AnyElement {
    let right_click = on_right_click(browser, pick);
    let target = link.target.clone();
    let preview = link.preview.as_ref();
    let leading = match preview.and_then(|preview| preview.picture.clone()) {
        Some(picture) => img(picture)
            .size(px(LINK_PICTURE))
            .flex_none()
            .rounded_lg()
            .object_fit(ObjectFit::Cover)
            .into_any_element(),
        None => badge(Icon::Link, LINK_PICTURE),
    };
    let title = preview.and_then(|preview| preview.title.clone());
    let about = preview
        .and_then(|preview| preview.description.clone())
        .or_else(|| {
            // A message that is just the link would only repeat it.
            let message = link.message.replace('\n', " ");
            (message.trim() != link.shown.as_ref()).then(|| message.into())
        });
    let place = format!("{} · {}", link.site, day(link.sent_at)).into();
    let below = about.into_iter().chain([place]).collect();
    entry(
        SharedString::from(format!("{}-{}", link.sent_at, link.target)),
        leading,
        title.clone().unwrap_or_else(|| link.shown.clone()),
        if title.is_some() { TEXT } else { LINK },
        below,
    )
    .on_mouse_down(RIGHT, right_click)
    .on_click(move |_, _, cx| cx.open_url(&target))
    .into_any_element()
}

/// The day `sent_at` falls on: "12 Nov".
fn day(sent_at: i64) -> String {
    Local
        .timestamp_opt(sent_at, 0)
        .single()
        .map_or_default(|time| time.format("%-d %b").to_string())
}
