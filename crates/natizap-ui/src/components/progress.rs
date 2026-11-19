//! How far something has gone, as a filled bar.

use gpui::prelude::*;
use gpui::{Div, div, px, relative, rgb};

use crate::theme::Theme;

/// A full-width bar filled to `fraction` (0 to 1) in the accent.
#[must_use]
pub fn progress(fraction: f32, theme: Theme) -> Div {
    div()
        .w_full()
        .h(px(4.0))
        .rounded_full()
        .bg(rgb(theme.line))
        .child(
            div()
                .h_full()
                .w(relative(fraction.clamp(0.0, 1.0)))
                .rounded_full()
                .bg(rgb(theme.accent)),
        )
}
