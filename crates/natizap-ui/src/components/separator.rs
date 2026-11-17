//! A hairline dividing two groups of content.

use gpui::prelude::*;
use gpui::{Div, div, px, rgb};

use crate::theme::LINE;

/// A full-width horizontal hairline.
#[must_use]
pub fn separator() -> Div {
    div().w_full().h(px(1.0)).flex_none().bg(rgb(LINE))
}
