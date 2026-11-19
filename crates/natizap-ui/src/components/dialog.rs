//! A modal layer: content centered over the dimmed window, rising into
//! place as the window dims.

use gpui::prelude::*;
use gpui::{App, Window, anchored, deferred, div, point, px, rgba};

use crate::motion::{fade_in, rise_in};
use crate::theme::Theme;

/// `content` centered over the whole window, dimmed behind it. Pressing the
/// dimmed part runs `on_dismiss`; `content` should occlude itself so presses
/// on it do not count.
pub fn dialog(
    window: &Window,
    content: impl IntoElement,
    on_dismiss: impl Fn(&mut Window, &mut App) + 'static,
    theme: Theme,
) -> impl IntoElement {
    let viewport = window.viewport_size();
    deferred(
        anchored().position(point(px(0.0), px(0.0))).child(fade_in(
            "dialog-scrim-in",
            div()
                .id("dialog-scrim")
                .w(viewport.width)
                .h(viewport.height)
                .flex()
                .items_center()
                .justify_center()
                .bg(rgba(theme.scrim))
                .occlude()
                .on_click(move |_, window, cx| on_dismiss(window, cx))
                .child(rise_in("dialog-in", div().child(content), 14.0)),
        )),
    )
    .with_priority(2)
}
