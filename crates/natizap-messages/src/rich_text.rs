//! Message text on screen: its markup styled, its code blocks set apart and
//! its links, email addresses and phone numbers clickable.

use std::ops::Range;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClipboardItem, CursorStyle, Div, ElementId, FontStyle, FontWeight,
    InteractiveText, MouseButton, SharedString, StrikethroughStyle, StyledText, TextLayout,
    TextRun, UnderlineStyle, WeakEntity, Window, div, px, rgb, rgba,
};
use natizap_protocol::{ChatId, LinkKind, Mark};
use natizap_ui::code_block;
use natizap_ui::theme::{
    ACCENT, ACCENT_INK, DEEP, LINK, MONO_FONT, MUTED, SELECTION, TEXT, base_font,
};

use crate::items::Text;
use crate::text_search::{Case, occurrences};
use crate::view::{ChatRequested, ConversationView};

/// Text selected in one stretch of a message, by dragging over it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Selection {
    /// Which stretch: unique among the texts on screen.
    pub(crate) key: SharedString,
    /// Where the drag started.
    anchor: usize,
    /// What is selected, as a byte range of `text`.
    pub(crate) range: Range<usize>,
    /// The stretch's text.
    pub(crate) text: SharedString,
}

impl Selection {
    /// The selected text, when any is.
    pub(crate) fn selected(&self) -> Option<&str> {
        Some(&self.text[self.range.clone()]).filter(|selected| !selected.is_empty())
    }
}

/// The link under the pointer, drawn lit up so it reads as clickable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HoveredLink {
    /// Which stretch of text holds it.
    key: SharedString,
    /// The link, as a byte range of the stretch.
    range: Range<usize>,
}

impl HoveredLink {
    /// Whether the link is in the text stretch `key`.
    pub(crate) fn is_in(&self, key: &str) -> bool {
        self.key == key
    }
}

/// What the texts on screen show lit: the selected text, the link the
/// pointer rests on, and what a search found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TextFocus {
    pub(crate) selection: Option<Selection>,
    pub(crate) hovered_link: Option<HoveredLink>,
    pub(crate) found: Option<Found>,
}

/// A search in the open conversation, lit in every message holding it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Found {
    pub(crate) query: SharedString,
    pub(crate) case: Case,
    /// The message the search is on, lit stronger than the rest.
    pub(crate) current: Option<SharedString>,
}

/// A stretch of text on screen that mouse drags select in.
#[derive(Clone)]
pub(crate) struct TextHit {
    key: SharedString,
    layout: TextLayout,
    text: SharedString,
}

/// Makes the texts in `hits`, drawn inside `container`, selectable by
/// dragging; the conversation keeps the selection.
pub(crate) fn selectable(
    container: Div,
    hits: Vec<TextHit>,
    view: &WeakEntity<ConversationView>,
) -> Div {
    if hits.is_empty() {
        return container;
    }
    let hits = Rc::new(hits);
    let (down_hits, down_view) = (hits.clone(), view.clone());
    let view = view.clone();
    container
        .cursor(CursorStyle::IBeam)
        .on_mouse_down(MouseButton::Left, move |event, _, cx| {
            // Only a press on a character starts a selection.
            let Some((hit, index)) = down_hits
                .iter()
                .find_map(|hit| Some((hit, hit.layout.index_for_position(event.position).ok()?)))
            else {
                return;
            };
            let selection = Selection {
                key: hit.key.clone(),
                anchor: index,
                range: index..index,
                text: hit.text.clone(),
            };
            let _ = down_view.update(cx, |view, cx| view.select_text(Some(selection), cx));
        })
        .on_mouse_move(move |event, _, cx| {
            if event.pressed_button != Some(MouseButton::Left) {
                return;
            }
            let _ = view.update(cx, |view, cx| {
                let Some(mut selection) = view.text_selection().cloned() else {
                    return;
                };
                let Some(hit) = hits.iter().find(|hit| hit.key == selection.key) else {
                    return;
                };
                let index = hit
                    .layout
                    .index_for_position(event.position)
                    .unwrap_or_else(|nearest| nearest)
                    .min(selection.text.len());
                selection.range = selection.anchor.min(index)..selection.anchor.max(index);
                view.select_text(Some(selection), cx);
            });
        })
}

/// `text` as paragraphs and code blocks, top to bottom, and the stretches
/// that can be selected. `id` must be unique among the bubbles on screen;
/// `selection` is shown where it falls.
pub(crate) fn rich_text(
    id: &str,
    text: &Text,
    view: &WeakEntity<ConversationView>,
    focus: &TextFocus,
) -> (AnyElement, Vec<TextHit>) {
    let shown = Shown { id, view, focus };
    let mut blocks = text
        .marks
        .iter()
        .filter(|(_, mark)| *mark == Mark::CodeBlock)
        .map(|(range, _)| range.clone())
        .collect::<Vec<_>>();
    blocks.sort_by_key(|range| range.start);
    // Plain text stays a bare text element: wrapped in a column, the
    // bubble's wrapping row squeezes it to one letter per line.
    if blocks.is_empty() {
        return match shown.prose(0, text, 0..text.text.len()) {
            Some((element, hit)) => (element.into_any_element(), vec![hit]),
            None => (div().into_any_element(), Vec::new()),
        };
    }
    let mut hits = Vec::new();
    let mut column = div().w_full().flex().flex_col().gap_1p5();
    let mut prose_start = 0;
    for (index, block) in blocks.iter().enumerate() {
        if let Some((element, hit)) = shown.prose(index, text, prose_start..block.start) {
            column = column.child(element);
            hits.push(hit);
        }
        column = column.child(code_block(text.text[block.clone()].to_owned()));
        prose_start = block.end;
    }
    if let Some((element, hit)) = shown.prose(blocks.len(), text, prose_start..text.text.len()) {
        column = column.child(element);
        hits.push(hit);
    }
    (column.into_any_element(), hits)
}

/// What every stretch of one message's text is drawn with.
struct Shown<'a> {
    id: &'a str,
    view: &'a WeakEntity<ConversationView>,
    focus: &'a TextFocus,
}

impl Shown<'_> {
    /// The text in `range` between code blocks, styled, or nothing when it
    /// is only line breaks.
    fn prose(
        &self,
        index: usize,
        text: &Text,
        range: Range<usize>,
    ) -> Option<(InteractiveText, TextHit)> {
        let slice = &text.text[range.clone()];
        let start = range.start + (slice.len() - slice.trim_start_matches('\n').len());
        let end = range.end - (slice.len() - slice.trim_end_matches('\n').len());
        if start >= end {
            return None;
        }
        let shown: SharedString = text.text[start..end].to_owned().into();
        let marks: Vec<(Range<usize>, Mark)> = text
            .marks
            .iter()
            .filter(|(mark_range, _)| mark_range.start < end && mark_range.end > start)
            .map(|(mark_range, mark)| {
                let clipped = mark_range.start.max(start) - start..mark_range.end.min(end) - start;
                (clipped, *mark)
            })
            .collect();
        let key: SharedString = format!("{}-text-{index}", self.id).into();
        let selected = self
            .focus
            .selection
            .as_ref()
            .filter(|selection| selection.key == key)
            .map(|selection| selection.range.clone());
        let hovered = self
            .focus
            .hovered_link
            .as_ref()
            .filter(|hovered| hovered.key == key)
            .map(|hovered| hovered.range.clone());
        let (found, current) = self.focus.found.as_ref().map_or_else(
            || (Vec::new(), false),
            |found| {
                (
                    occurrences(&shown, &found.query, found.case),
                    found.current.as_deref() == Some(self.id),
                )
            },
        );
        let lit = Lit {
            selected,
            hovered,
            found: &found,
            current,
        };
        let runs = runs(shown.len(), &marks, &lit);
        let links: Vec<(Range<usize>, LinkKind)> = marks
            .iter()
            .filter_map(|(range, mark)| match mark {
                Mark::Link(kind) => Some((range.clone(), *kind)),
                _ => None,
            })
            .collect();
        let targets: Vec<(LinkKind, String)> = links
            .iter()
            .map(|(range, kind)| (*kind, kind.target(&shown[range.clone()])))
            .collect();
        let view = self.view.clone();
        let styled = StyledText::new(shown.clone()).with_runs(runs);
        let hit = TextHit {
            key: key.clone(),
            layout: styled.layout().clone(),
            text: shown,
        };
        let ranges: Vec<Range<usize>> = links.into_iter().map(|(range, _)| range).collect();
        let (hover_view, hover_key, hover_ranges) = (view.clone(), key.clone(), ranges.clone());
        let element = InteractiveText::new(ElementId::Name(key), styled)
            .on_click(ranges, move |clicked, _: &mut Window, cx: &mut App| {
                if let Some((kind, target)) = targets.get(clicked) {
                    open(*kind, target, &view, cx);
                }
            })
            .on_hover(move |index, _, _, cx| {
                let link = index.and_then(|index| {
                    hover_ranges
                        .iter()
                        .find(|range| range.contains(&index))
                        .map(|range| HoveredLink {
                            key: hover_key.clone(),
                            range: range.clone(),
                        })
                });
                let key = hover_key.clone();
                let _ = hover_view.update(cx, |view, cx| view.hover_link(&key, link, cx));
            });
        Some((element, hit))
    }
}

fn open(kind: LinkKind, target: &str, view: &WeakEntity<ConversationView>, cx: &mut App) {
    match kind {
        LinkKind::Web | LinkKind::Email => cx.open_url(target),
        LinkKind::TaxId => cx.write_to_clipboard(ClipboardItem::new_string(target.to_owned())),
        LinkKind::Phone => {
            let phone = ChatId::new(&format!("{target}@s.whatsapp.net"));
            let _ = view.update(cx, |_, cx| cx.emit(ChatRequested { phone }));
        }
    }
}

/// What lights a stretch of text up, besides its marks.
#[derive(Debug, Default)]
struct Lit<'a> {
    /// The text selected in it.
    selected: Option<Range<usize>>,
    /// The link the pointer rests on.
    hovered: Option<Range<usize>>,
    /// What a search found in it.
    found: &'a [Range<usize>],
    /// Whether the search is on this message.
    current: bool,
}

/// One text run per stretch of `len` bytes that `marks` style alike, with
/// the selected text behind a highlight, the hovered link lit up and what
/// a search found marked.
fn runs(len: usize, marks: &[(Range<usize>, Mark)], lit: &Lit<'_>) -> Vec<TextRun> {
    let selected = lit.selected.clone().filter(|range| !range.is_empty());
    let hovered = lit.hovered.clone().filter(|range| !range.is_empty());
    let mut edges: Vec<usize> = marks
        .iter()
        .flat_map(|(range, _)| [range.start, range.end])
        .chain(selected.iter().flat_map(|range| [range.start, range.end]))
        .chain(hovered.iter().flat_map(|range| [range.start, range.end]))
        .chain(lit.found.iter().flat_map(|range| [range.start, range.end]))
        .chain([0, len])
        .filter(|&edge| edge <= len)
        .collect();
    edges.sort_unstable();
    edges.dedup();
    edges
        .windows(2)
        .map(|edge| {
            let covering = marks
                .iter()
                .filter(|(range, _)| range.start <= edge[0] && range.end >= edge[1])
                .map(|(_, mark)| *mark);
            let mut run = run(edge[1] - edge[0], covering);
            let covers = |range: &Range<usize>| range.start <= edge[0] && range.end >= edge[1];
            if lit.found.iter().any(covers) {
                // The message the search is on stands out from the others
                // holding the same words.
                let alpha = if lit.current { 0xb0 } else { 0x48 };
                run.background_color = Some(rgba((ACCENT << 8) | alpha).into());
                if lit.current {
                    run.color = rgb(ACCENT_INK).into();
                }
            }
            if hovered
                .as_ref()
                .is_some_and(|range| range.start <= edge[0] && range.end >= edge[1])
            {
                run.color = rgb(ACCENT).into();
                run.background_color = Some(rgba((ACCENT << 8) | 0x24).into());
            }
            if selected
                .as_ref()
                .is_some_and(|range| range.start <= edge[0] && range.end >= edge[1])
            {
                run.background_color = Some(rgba(SELECTION).into());
            }
            run
        })
        .collect()
}

fn run(len: usize, marks: impl Iterator<Item = Mark>) -> TextRun {
    let mut font = base_font();
    let mut run = TextRun {
        len,
        font: font.clone(),
        color: rgb(TEXT).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    for mark in marks {
        match mark {
            Mark::Bold => font.weight = FontWeight::BOLD,
            Mark::Italic => font.style = FontStyle::Italic,
            Mark::Strikethrough => {
                run.strikethrough = Some(StrikethroughStyle {
                    thickness: px(1.0),
                    color: None,
                });
            }
            Mark::Code | Mark::CodeBlock => {
                font.family = MONO_FONT.into();
                run.background_color = Some(rgb(DEEP).into());
            }
            Mark::Quote => run.color = rgb(MUTED).into(),
            Mark::Link(_) => {
                run.color = rgb(LINK).into();
                run.underline = Some(UnderlineStyle {
                    thickness: px(1.0),
                    color: None,
                    wavy: false,
                });
            }
        }
    }
    run.font = font;
    run
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_text_where_its_styles_change() {
        let marks = [(0..4, Mark::Bold), (2..8, Mark::Link(LinkKind::Web))];
        let runs: Vec<(usize, bool, bool)> = runs(10, &marks, &Lit::default())
            .iter()
            .map(|run| {
                (
                    run.len,
                    run.font.weight == FontWeight::BOLD,
                    run.underline.is_some(),
                )
            })
            .collect();
        assert_eq!(
            runs,
            [
                (2, true, false),
                (2, true, true),
                (4, false, true),
                (2, false, false)
            ]
        );
    }

    #[test]
    fn lights_up_only_the_hovered_link() {
        let marks = [
            (0..4, Mark::Link(LinkKind::Web)),
            (6..10, Mark::Link(LinkKind::Web)),
        ];
        let hovering = Lit {
            hovered: Some(6..10),
            ..Lit::default()
        };
        let lit: Vec<(usize, bool)> = runs(10, &marks, &hovering)
            .iter()
            .map(|run| (run.len, run.background_color.is_some()))
            .collect();
        assert_eq!(lit, [(4, false), (2, false), (4, true)]);
    }

    #[test]
    fn marks_what_a_search_found() {
        let found = [Range { start: 2, end: 5 }];
        let searching = Lit {
            found: &found,
            ..Lit::default()
        };
        let marked: Vec<(usize, bool)> = runs(8, &[], &searching)
            .iter()
            .map(|run| (run.len, run.background_color.is_some()))
            .collect();
        assert_eq!(marked, [(2, false), (3, true), (3, false)]);
    }
}
