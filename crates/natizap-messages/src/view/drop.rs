//! Files dragged over the conversation: dropping them shows them before
//! sending.

use gpui::prelude::*;
use gpui::{Div, ExternalPaths, FontWeight, div, px, rgb, svg};
use natizap_ui::Icon;
use natizap_ui::theme::{ACCENT, PANE};

/// What shows while files are dragged over the conversation: dropping them
/// shows them before sending.
pub(super) fn drop_zone() -> Div {
    div()
        .absolute()
        .inset_0()
        .m_3()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_3()
        .rounded_xl()
        .border_2()
        .border_dashed()
        .border_color(rgb(ACCENT))
        .bg(rgb(PANE))
        .opacity(0.94)
        .invisible()
        .group_drag_over::<ExternalPaths>("conversation", Styled::visible)
        .child(
            svg()
                .path(Icon::Plus.path())
                .size(px(40.0))
                .text_color(rgb(ACCENT)),
        )
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .child("Drop files to send them"),
        )
}
