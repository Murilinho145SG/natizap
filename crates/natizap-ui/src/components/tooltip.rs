//! A short label that names a control while the pointer rests on it.

use gpui::prelude::*;
use gpui::{AnyView, App, Context, SharedString, Window, div, px, rgb};

use crate::motion::fade_in;
use crate::theme::ActiveTheme as _;

/// The floating label itself; build one with [`Tooltip::text`].
#[derive(Debug)]
pub struct Tooltip {
    text: SharedString,
}

impl Tooltip {
    /// A tooltip builder for `.tooltip(...)` on any element with an id.
    pub fn text(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView {
        let text = text.into();
        move |_, cx| cx.new(|_| Self { text: text.clone() }).into()
    }
}

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        fade_in(
            "tooltip-in",
            div()
                .px_2()
                .py_1()
                .rounded_md()
                .bg(rgb(theme.panel))
                .map(|surface| theme.glass(surface))
                .text_size(px(12.0))
                .text_color(rgb(theme.text))
                .child(self.text.clone()),
        )
    }
}
