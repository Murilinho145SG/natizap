//! A slider: a track with a knob, dragged or clicked anywhere along it to
//! choose a value between its ends.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    App, Bounds, DispatchPhase, ElementId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, Window, canvas, div, px, relative, rgb,
};

use crate::theme::ActiveTheme as _;

type ChangeHandler = Rc<dyn Fn(f32, &mut Window, &mut App)>;

/// The knob's size.
const KNOB: f32 = 14.0;

/// A track filled up to its value, a fraction from 0 to 1, with a knob
/// there. Pressing it moves the knob where it was pressed, and it follows
/// the pointer, even past its ends, until let go; each move reports the
/// fraction asked for, which the caller decides whether to apply.
#[derive(IntoElement)]
pub struct Slider {
    id: ElementId,
    value: f32,
    mark: Option<f32>,
    on_change: Option<ChangeHandler>,
}

impl std::fmt::Debug for Slider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Slider")
            .field("id", &self.id)
            .field("value", &self.value)
            .finish_non_exhaustive()
    }
}

impl Slider {
    /// A slider at `value`, a fraction from 0 to 1.
    pub fn new(id: impl Into<ElementId>, value: f32) -> Self {
        Self {
            id: id.into(),
            value: value.clamp(0.0, 1.0),
            mark: None,
            on_change: None,
        }
    }

    /// A tick at `fraction` along the track: a value worth finding again,
    /// such as where nothing is changed.
    #[must_use]
    pub fn mark(mut self, fraction: f32) -> Self {
        self.mark = Some(fraction.clamp(0.0, 1.0));
        self
    }

    /// Runs with the fraction a press or a drag asks for.
    #[must_use]
    pub fn on_change(mut self, handler: impl Fn(f32, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

/// What a slider keeps between frames: where it was drawn, and whether
/// the pointer holds it.
#[derive(Default)]
struct Held {
    bounds: Option<Bounds<Pixels>>,
    holding: bool,
}

/// The fraction `at` stands for along `bounds`.
fn fraction(bounds: Bounds<Pixels>, at: Point<Pixels>) -> f32 {
    if bounds.size.width <= px(0.0) {
        return 0.0;
    }
    ((at.x - bounds.origin.x) / bounds.size.width).clamp(0.0, 1.0)
}

impl RenderOnce for Slider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.theme();
        let held = window.use_keyed_state(self.id.clone(), cx, |_, _| Held::default());
        let holding = held.read(cx).holding;
        let value = self.value;
        let on_change = self.on_change;
        let pressing = (held.clone(), on_change.clone());
        let following = (held.clone(), on_change);
        div()
            .id(self.id)
            .relative()
            .w_full()
            .h(px(20.0))
            .flex()
            .items_center()
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                move |event: &MouseDownEvent, window, cx| {
                    let (held, on_change) = &pressing;
                    cx.stop_propagation();
                    let bounds = held.update(cx, |held, cx| {
                        held.holding = true;
                        cx.notify();
                        held.bounds
                    });
                    if let (Some(bounds), Some(on_change)) = (bounds, on_change) {
                        on_change(fraction(bounds, event.position), window, cx);
                    }
                },
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(4.0))
                    .rounded_full()
                    .bg(rgb(theme.line))
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top_0()
                            .h_full()
                            .w(relative(value))
                            .rounded_full()
                            .bg(rgb(theme.accent)),
                    )
                    .children(self.mark.map(|mark| {
                        div()
                            .absolute()
                            .left(relative(mark))
                            .top(px(-4.0))
                            .w(px(2.0))
                            .h(px(12.0))
                            .ml(px(-1.0))
                            .rounded_full()
                            .bg(rgb(theme.muted))
                    })),
            )
            .child(
                div()
                    .absolute()
                    .left(relative(value))
                    .ml(px(-KNOB / 2.0))
                    .size(px(KNOB))
                    .rounded_full()
                    .bg(rgb(theme.text))
                    .shadow_sm()
                    .when(holding, |knob| {
                        knob.border_2().border_color(rgb(theme.accent))
                    }),
            )
            .child(
                canvas(
                    {
                        let held = held.clone();
                        move |bounds, _, cx| held.update(cx, |held, _| held.bounds = Some(bounds))
                    },
                    move |_, (), window, _| {
                        if holding {
                            follow(window, following.clone());
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
    }
}

/// While the slider is held, follows the pointer anywhere in the window,
/// and lets go when the button is released.
fn follow(window: &mut Window, (held, on_change): (gpui::Entity<Held>, Option<ChangeHandler>)) {
    let moving = held.clone();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
        if phase != DispatchPhase::Bubble {
            return;
        }
        let state = moving.read(cx);
        let (holding, bounds) = (state.holding, state.bounds);
        if !holding {
            return;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            // Released outside the window, where its release is not heard.
            moving.update(cx, |held, cx| {
                held.holding = false;
                cx.notify();
            });
            return;
        }
        if let (Some(bounds), Some(on_change)) = (bounds, &on_change) {
            on_change(fraction(bounds, event.position), window, cx);
        }
    });
    window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
        if phase == DispatchPhase::Bubble {
            held.update(cx, |held, cx| {
                held.holding = false;
                cx.notify();
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use gpui::{Bounds, point, px, size};

    use super::fraction;

    #[test]
    fn a_press_stands_for_where_it_is_along_the_track() {
        let track = Bounds::new(point(px(100.0), px(0.0)), size(px(200.0), px(20.0)));
        assert!((fraction(track, point(px(150.0), px(5.0))) - 0.25).abs() < f32::EPSILON);
        // Past either end, the end.
        assert!(fraction(track, point(px(40.0), px(5.0))).abs() < f32::EPSILON);
        assert!((fraction(track, point(px(900.0), px(5.0))) - 1.0).abs() < f32::EPSILON);
    }
}
