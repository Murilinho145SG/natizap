//! A hairline dividing two groups of content.

use gpui::prelude::*;
use gpui::{Div, div, px, rgb};

use crate::theme::Theme;

/// A full-width horizontal hairline.
#[must_use]
pub fn separator(theme: Theme) -> Div {
    div().w_full().h(px(1.0)).flex_none().bg(rgb(theme.line))
}
