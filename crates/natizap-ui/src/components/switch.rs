//! An on/off toggle.

use gpui::prelude::*;
use gpui::{Animation, AnimationExt as _, App, ElementId, Window, div, px, rgb};

use crate::motion;
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

/// What a switch showed last, so a change can slide the knob across.
struct Shown {
    on: bool,
    /// How many times it changed while on screen; names the slide, so each
    /// change plays it once.
    changes: u32,
}

/// How far the knob travels: the track's inner width less the knob.
const TRAVEL: f32 = 38.0 - 2.0 * 3.0 - 16.0;

impl RenderOnce for Switch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let on = self.on;
        let shown = window.use_keyed_state(self.id.clone(), cx, |_, _| Shown { on, changes: 0 });
        let changes = shown.update(cx, |shown, _| {
            if shown.on != on {
                shown.on = on;
                shown.changes += 1;
            }
            shown.changes
        });
        // It came from the other end, so it starts there.
        let from = if on { -TRAVEL } else { TRAVEL };
        let knob = div().size(px(16.0)).rounded_full().bg(rgb(TEXT));
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
            .map(|switch| {
                if changes == 0 {
                    switch.child(knob)
                } else {
                    switch.child(knob.with_animation(
                        ("knob", changes),
                        Animation::new(motion::QUICK).with_easing(motion::ease_out),
                        move |knob, delta| knob.relative().left(px(from * (1.0 - delta))),
                    ))
                }
            })
    }
}
