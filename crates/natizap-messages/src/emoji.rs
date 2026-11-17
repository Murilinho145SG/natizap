//! The composer's emoji picker.

use gpui::prelude::*;
use gpui::{Div, Stateful, div, px, rgb};
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

/// A scrollable grid of [`PICKER`] emoji; `on_pick` gets the chosen one.
pub(crate) fn picker(
    on_pick: impl Fn(&'static str, &mut gpui::Window, &mut gpui::App) + Clone + 'static,
) -> Stateful<Div> {
    div()
        .id("emoji-picker")
        // Without scrolling the rows past the height limit spill over the
        // composer below.
        .overflow_y_scroll()
        .flex()
        .flex_wrap()
        .content_start()
        .gap_0p5()
        .p_2()
        // A fixed size keeps the floating panel steady while it scrolls.
        .h(px(220.0))
        .flex_none()
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
                .text_size(px(22.0))
                .child(emoji)
        }))
}
