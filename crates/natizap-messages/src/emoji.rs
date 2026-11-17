//! The composer's emoji picker.

use gpui::prelude::*;
use gpui::{Div, div, px, rgb};
use natizap_ui::theme::{LINE, PANEL, RAISED};

/// The emoji people reach for most, in roughly that order; a full catalog
/// with search can replace this list later.
pub(crate) const PICKER: &[&str] = &[
    "😂", "❤️", "🤣", "👍", "😭", "🙏", "😘", "🥰", "😍", "😊", "🎉", "😁", "💕", "🥺", "😅", "🔥",
    "☺️", "🤦", "♥️", "🤷", "🙄", "😆", "🤗", "😉", "🎂", "🤔", "👏", "🙂", "😳", "🥳", "😎", "👌",
    "💜", "😔", "💪", "✨", "💖", "👀", "😋", "😏", "😢", "👉", "💗", "😩", "💯", "🌹", "💞", "🎈",
    "💙", "😃", "😡", "💐", "😜", "🙈", "🤞", "😄", "🤤", "🙌", "🤪", "❣️", "😀", "💋", "💀", "👇",
    "💔", "😌", "💓", "🤩", "🙃", "😬", "😱", "😴", "🤭", "😐", "🌞", "😒", "😇", "🌸", "😈", "🎶",
    "✌️", "🎊", "🥵", "😞", "💚", "☀️", "🖤", "💰", "😚", "👑",
];

/// A grid of [`PICKER`] emoji; `on_pick` gets the chosen one.
pub(crate) fn picker(
    on_pick: impl Fn(&'static str, &mut gpui::Window, &mut gpui::App) + Clone + 'static,
) -> Div {
    div()
        .flex()
        .flex_wrap()
        .gap_0p5()
        .p_2()
        .max_h(px(220.0))
        .rounded_xl()
        .bg(rgb(PANEL))
        .border_1()
        .border_color(rgb(LINE))
        .children(PICKER.iter().map(|&emoji| {
            let on_pick = on_pick.clone();
            div()
                .id(emoji)
                .size(px(36.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .cursor_pointer()
                .hover(|style| style.bg(rgb(RAISED)))
                .on_click(move |_, window, cx| on_pick(emoji, window, cx))
                .child(natizap_ui::emoji(emoji, px(24.0)))
        }))
}
