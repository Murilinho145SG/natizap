//! How far something has gone, as a filled bar.

use gpui::prelude::*;
use gpui::{Div, div, px, relative, rgb};

use crate::theme::{ACCENT, LINE};

/// A full-width bar filled to `fraction` (0 to 1) in the accent.
#[must_use]
pub fn progress(fraction: f32) -> Div {
    div()
        .w_full()
        .h(px(4.0))
        .rounded_full()
        .bg(rgb(LINE))
        .child(
            div()
                .h_full()
                .w(relative(fraction.clamp(0.0, 1.0)))
                .rounded_full()
                .bg(rgb(ACCENT)),
        )
}
