//! Message text on screen: its markup styled, its code blocks set apart and
//! its links, email addresses and phone numbers clickable.

use std::ops::Range;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClipboardItem, ElementId, FontStyle, FontWeight, InteractiveText,
    StrikethroughStyle, StyledText, TextRun, UnderlineStyle, WeakEntity, Window, div, px, rgb,
};
use natizap_protocol::{ChatId, LinkKind, Mark};
use natizap_ui::code_block;
use natizap_ui::theme::{DEEP, LINK, MONO_FONT, MUTED, TEXT, base_font};

use crate::items::Text;
use crate::view::{ChatRequested, ConversationView};

/// `text` as paragraphs and code blocks, top to bottom. `id` must be unique
/// among the bubbles on screen.
pub(crate) fn rich_text(id: &str, text: &Text, view: &WeakEntity<ConversationView>) -> AnyElement {
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
        return prose(id, 0, text, 0..text.text.len(), view)
            .map_or_else(|| div().into_any_element(), IntoElement::into_any_element);
    }
    let mut column = div().w_full().flex().flex_col().gap_1p5();
    let mut prose_start = 0;
    for (index, block) in blocks.iter().enumerate() {
        column = column.children(prose(id, index, text, prose_start..block.start, view));
        column = column.child(code_block(text.text[block.clone()].to_owned()));
        prose_start = block.end;
    }
    let rest = prose_start..text.text.len();
    column
        .children(prose(id, blocks.len(), text, rest, view))
        .into_any_element()
}

/// The text in `range` between code blocks, styled, or nothing when it is
/// only line breaks.
fn prose(
    id: &str,
    index: usize,
    text: &Text,
    range: Range<usize>,
    view: &WeakEntity<ConversationView>,
) -> Option<InteractiveText> {
    let slice = &text.text[range.clone()];
    let start = range.start + (slice.len() - slice.trim_start_matches('\n').len());
    let end = range.end - (slice.len() - slice.trim_end_matches('\n').len());
    if start >= end {
        return None;
    }
    let shown = text.text[start..end].to_owned();
    let marks: Vec<(Range<usize>, Mark)> = text
        .marks
        .iter()
        .filter(|(mark_range, _)| mark_range.start < end && mark_range.end > start)
        .map(|(mark_range, mark)| {
            let clipped = mark_range.start.max(start) - start..mark_range.end.min(end) - start;
            (clipped, *mark)
        })
        .collect();
    let runs = runs(shown.len(), &marks);
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
    let view = view.clone();
    let element_id = ElementId::Name(format!("{id}-text-{index}").into());
    Some(
        InteractiveText::new(element_id, StyledText::new(shown).with_runs(runs)).on_click(
            links.into_iter().map(|(range, _)| range).collect(),
            move |clicked, _: &mut Window, cx: &mut App| {
                if let Some((kind, target)) = targets.get(clicked) {
                    open(*kind, target, &view, cx);
                }
            },
        ),
    )
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

/// One text run per stretch of `len` bytes that `marks` style alike.
fn runs(len: usize, marks: &[(Range<usize>, Mark)]) -> Vec<TextRun> {
    let mut edges: Vec<usize> = marks
        .iter()
        .flat_map(|(range, _)| [range.start, range.end])
        .chain([0, len])
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
            run(edge[1] - edge[0], covering)
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
        let runs: Vec<(usize, bool, bool)> = runs(10, &marks)
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
}
