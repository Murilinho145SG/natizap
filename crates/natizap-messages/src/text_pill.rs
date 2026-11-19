//! The rounded glass field that holds a message being written: the
//! composer's and the caption's.

use gpui::prelude::*;
use gpui::{App, Div, ElementId, Entity, Window, div, px, rgb};
use natizap_ui::theme::ActiveTheme as _;
use natizap_ui::{TextInput, scrollbar};

/// `input` in a pill that grows with its wrapped lines, with a scrollbar
/// in its edge once it grows past its tallest.
pub(crate) fn text_pill(
    id: impl Into<ElementId>,
    input: &Entity<TextInput>,
    window: &mut Window,
    cx: &mut App,
) -> Div {
    let theme = *cx.theme();
    div()
        .relative()
        .px_4()
        .py_2p5()
        // Pill-round on one line, softer as wrapped lines grow it.
        .rounded(px(22.0))
        .bg(rgb(theme.raised))
        .map(|surface| theme.glass(surface))
        .text_size(px(14.5))
        .text_color(rgb(theme.text))
        .child(input.clone())
        // Held clear of the rounded corners.
        .child(
            scrollbar(id, input, window, cx)
                .top(px(12.0))
                .bottom(px(12.0)),
        )
}
