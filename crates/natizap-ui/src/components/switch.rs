//! An on/off toggle.

use gpui::prelude::*;
use gpui::{App, ElementId, Window, div, px, rgb};

use crate::theme::{ACCENT, LINE, TEXT};

type ToggleHandler = Box<dyn Fn(bool, &mut Window, &mut App)>;

/// A track with a knob, accent-filled when on. Clicking it reports the new
/// state; the caller decides whether to apply it.
#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    on: bool,
    on_toggle: Option<ToggleHandler>,
}

impl std::fmt::Debug for Switch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Switch")
            .field("id", &self.id)
            .field("on", &self.on)
            .finish_non_exhaustive()
    }
}

impl Switch {
    /// A switch showing `on`.
    pub fn new(id: impl Into<ElementId>, on: bool) -> Self {
        Self {
            id: id.into(),
            on,
            on_toggle: None,
        }
    }

    /// Runs with the state a click asks for.
    #[must_use]
    pub fn on_toggle(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Switch {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let on = self.on;
        div()
            .id(self.id)
            .w(px(38.0))
            .h(px(22.0))
            .flex_none()
            .flex()
            .items_center()
            .px(px(3.0))
            .rounded_full()
            .bg(rgb(if on { ACCENT } else { LINE }))
            .cursor_pointer()
            .when(on, Styled::justify_end)
            .when_some(self.on_toggle, |switch, handler| {
                switch.on_click(move |_, window, cx| handler(!on, window, cx))
            })
            .child(div().size(px(16.0)).rounded_full().bg(rgb(TEXT)))
    }
}
