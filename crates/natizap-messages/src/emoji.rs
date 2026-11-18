//! The emoji picker: every emoji by category, with a sidebar to jump
//! between categories. The composer's tray and the reaction popover show it.

use std::ops::Range;
use std::sync::LazyLock;

use emojis::{Group, SkinTone, UnicodeVersion};
use gpui::prelude::*;
use gpui::{
    Context, EventEmitter, FontWeight, ScrollStrategy, UniformListScrollHandle, Window, div, px,
    rgb, uniform_list,
};
use natizap_ui::Tooltip;
use natizap_ui::theme::{MUTED, RAISED};

/// The emoji people reach for most, in roughly that order: the first
/// category, as on the phone.
const FREQUENT: &[&str] = &[
    "😂", "❤️", "🤣", "👍", "😭", "🙏", "😘", "🥰", "😍", "😊", "🎉", "😁", "💕", "🥺", "😅", "🔥",
    "☺️", "🤦", "♥️", "🤷", "🙄", "😆", "🤗", "😉", "🎂", "🤔", "👏", "🙂", "😳", "🥳", "😎", "👌",
];

/// Newest emoji the bundled font draws; later ones would show as boxes.
const FONT_COVERS: UnicodeVersion = UnicodeVersion::new(15, 1);

const COLUMNS: u16 = 8;
const CELL: f32 = 38.0;
const SIDEBAR: f32 = 44.0;
const PADDING: f32 = 8.0;
/// Width of the picker, sized from the grid so no column of empty space is
/// left over.
pub(crate) const WIDTH: f32 = SIDEBAR + COLUMNS as f32 * CELL + 2.0 * PADDING;
/// Height the picker is given where it stands alone.
pub(crate) const HEIGHT: f32 = 320.0;

/// One category: its name, the emoji standing for it in the sidebar, and
/// its emoji.
struct Category {
    name: &'static str,
    icon: &'static str,
    emoji: Vec<&'static str>,
}

/// A line of the scrolling list: a category's title or a row of emoji.
enum Line {
    Title(usize),
    Emoji(Range<usize>, usize),
}

static CATEGORIES: LazyLock<Vec<Category>> = LazyLock::new(|| {
    let mut categories = vec![Category {
        name: "Frequently used",
        icon: "🕘",
        emoji: FREQUENT.to_vec(),
    }];
    categories.extend(Group::iter().map(|group| {
        let (name, icon) = match group {
            Group::SmileysAndEmotion => ("Smileys & emotion", "😀"),
            Group::PeopleAndBody => ("People & body", "👋"),
            Group::AnimalsAndNature => ("Animals & nature", "🐻"),
            Group::FoodAndDrink => ("Food & drink", "🍔"),
            Group::TravelAndPlaces => ("Travel & places", "✈️"),
            Group::Activities => ("Activities", "⚽"),
            Group::Objects => ("Objects", "💡"),
            Group::Symbols => ("Symbols", "💕"),
            Group::Flags => ("Flags", "🏁"),
        };
        let emoji = group
            .emojis()
            .filter(|emoji| emoji.unicode_version() <= FONT_COVERS)
            .filter(|emoji| {
                emoji
                    .skin_tone()
                    .is_none_or(|tone| tone == SkinTone::Default)
            })
            .map(emojis::Emoji::as_str)
            .collect();
        Category { name, icon, emoji }
    }));
    categories
});

/// Every category laid out as lines of the list, and the line each
/// category starts at.
static LINES: LazyLock<(Vec<Line>, Vec<usize>)> = LazyLock::new(|| {
    let mut lines = Vec::new();
    let mut starts = Vec::new();
    for (index, category) in CATEGORIES.iter().enumerate() {
        starts.push(lines.len());
        lines.push(Line::Title(index));
        let count = category.emoji.len();
        lines.extend(
            (0..count)
                .step_by(usize::from(COLUMNS))
                .map(|start| Line::Emoji(start..(start + usize::from(COLUMNS)).min(count), index)),
        );
    }
    (lines, starts)
});

/// The user picked an emoji.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EmojiPicked(pub(crate) &'static str);

/// The picker panel; it emits [`EmojiPicked`].
#[derive(Debug)]
pub(crate) struct EmojiPicker {
    scroll: UniformListScrollHandle,
}

impl EventEmitter<EmojiPicked> for EmojiPicker {}

impl EmojiPicker {
    pub(crate) fn new() -> Self {
        Self {
            scroll: UniformListScrollHandle::new(),
        }
    }

    /// The category whose title is at or above the top of the list.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a line index, clamped at zero and far below usize's range"
    )]
    fn current_category(&self) -> usize {
        let (lines, _) = &*LINES;
        // Every line is one cell tall, so the scroll offset says which line
        // is at the top. The handle's own top-index lookup is test-only.
        let scrolled = -self.scroll.0.borrow().base_handle.offset().y;
        let top = (scrolled / px(CELL)).max(0.0) as usize;
        match lines.get(top) {
            Some(Line::Title(category) | Line::Emoji(_, category)) => *category,
            None => 0,
        }
    }
}

impl Render for EmojiPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.current_category();
        let sidebar = div()
            .w(px(SIDEBAR))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap_0p5()
            .py_2()
            .border_r_1()
            .border_color(gpui::rgba(natizap_ui::theme::EDGE))
            .children(CATEGORIES.iter().enumerate().map(|(index, category)| {
                div()
                    .id(("emoji-category", index))
                    .size(px(32.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .cursor_pointer()
                    .text_size(px(18.0))
                    .when(index == current, |icon| icon.bg(rgb(RAISED)))
                    .when(index != current, |icon| icon.opacity(0.6))
                    .hover(|icon| icon.bg(rgb(RAISED)).opacity(1.0))
                    .tooltip(Tooltip::text(category.name))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let (_, starts) = &*LINES;
                        this.scroll
                            .scroll_to_item_strict(starts[index], ScrollStrategy::Top);
                        cx.notify();
                    }))
                    .child(category.icon)
            }));
        let list = uniform_list(
            "emoji",
            LINES.0.len(),
            cx.processor(|_, range: Range<usize>, _, cx| {
                LINES.0[range]
                    .iter()
                    .map(|line| match line {
                        Line::Title(category) => div()
                            .h(px(CELL))
                            .px_1()
                            .flex()
                            .items_end()
                            .pb_1()
                            .text_size(px(12.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(MUTED))
                            .child(CATEGORIES[*category].name)
                            .into_any_element(),
                        Line::Emoji(range, category) => div()
                            .h(px(CELL))
                            .flex()
                            .children(CATEGORIES[*category].emoji[range.clone()].iter().map(
                                |&emoji| {
                                    div()
                                        .id(emoji)
                                        .size(px(CELL))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded_md()
                                        .cursor_pointer()
                                        .hover(|cell| cell.bg(rgb(RAISED)))
                                        .on_click(cx.listener(move |_, _, _, cx| {
                                            cx.emit(EmojiPicked(emoji));
                                        }))
                                        .text_size(px(22.0))
                                        .child(emoji)
                                },
                            ))
                            .into_any_element(),
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.scroll)
        .h_full()
        .w(px(f32::from(COLUMNS) * CELL));
        // Unframed: the tray and the reaction popover each frame it.
        div()
            .id("emoji-picker")
            .size_full()
            .flex()
            // Scrolling moves the current category in the sidebar.
            .on_scroll(cx.listener(|_, _, _, cx| cx.notify()))
            .child(sidebar)
            .child(div().flex_1().h_full().px(px(PADDING)).py_1().child(list))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_category_starts_at_its_title_and_keeps_all_its_emoji() {
        let (lines, starts) = &*LINES;
        assert_eq!(starts.len(), CATEGORIES.len());
        for (index, category) in CATEGORIES.iter().enumerate() {
            assert!(!category.emoji.is_empty(), "{} is empty", category.name);
            assert!(matches!(lines[starts[index]], Line::Title(title) if title == index));
            let laid_out: usize = lines
                .iter()
                .filter_map(|line| match line {
                    Line::Emoji(range, owner) if *owner == index => Some(range.len()),
                    _ => None,
                })
                .sum();
            assert_eq!(laid_out, category.emoji.len());
        }
    }
}
