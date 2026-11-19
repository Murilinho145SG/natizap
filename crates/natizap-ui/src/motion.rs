//! How things move. Every animation in natizap takes its timing and curve
//! from here, so the whole app moves alike and changes in one place.
//!
//! GPUI has no style transitions, only animations keyed by an element id:
//! an animation runs once each time an element with its id first shows,
//! so a new id (a newly opened chat, a toggled switch) plays it again.

use std::time::Duration;

use gpui::prelude::*;
use gpui::{Animation, AnimationElement, AnimationExt as _, ElementId, px};

/// For things that answer the user at once: a toggle, a press.
pub const QUICK: Duration = Duration::from_millis(140);
/// For things that appear: menus, dialogs, panels, messages.
pub const ENTER: Duration = Duration::from_millis(220);

/// One breath of something that pulses while a state lasts: dots while
/// someone types, a ring while a call rings.
pub const BREATH: Duration = Duration::from_millis(1200);

/// How far into a breath `delta` is, as a swell: 0 at its ends, 1 halfway.
#[must_use]
pub fn swell(delta: f32) -> f32 {
    1.0 - (delta * 2.0 - 1.0).abs()
}

/// Starts fast and settles gently, as things that arrive should.
#[must_use]
pub fn ease_out(delta: f32) -> f32 {
    1.0 - (1.0 - delta).powi(3)
}

/// `element` fading in.
pub fn fade_in<E: IntoElement + Styled + 'static>(
    id: impl Into<ElementId>,
    element: E,
) -> AnimationElement<E> {
    element.with_animation(
        id,
        Animation::new(ENTER).with_easing(ease_out),
        Styled::opacity,
    )
}

/// `element` fading in while it rises `distance` pixels into place. The
/// element is positioned relatively for it, so it must not be absolute.
pub fn rise_in<E: IntoElement + Styled + 'static>(
    id: impl Into<ElementId>,
    element: E,
    distance: f32,
) -> AnimationElement<E> {
    element.with_animation(
        id,
        Animation::new(ENTER).with_easing(ease_out),
        move |element, delta| {
            element
                .opacity(delta)
                .relative()
                .top(px(distance * (1.0 - delta)))
        },
    )
}

/// [`rise_in`] starting `delay` later: given growing delays, the items of
/// a list rise one after another.
pub fn rise_in_after<E: IntoElement + Styled + 'static>(
    id: impl Into<ElementId>,
    element: E,
    distance: f32,
    delay: Duration,
) -> AnimationElement<E> {
    let total = delay + ENTER;
    let waiting = delay.as_secs_f32() / total.as_secs_f32();
    element.with_animation(
        id,
        Animation::new(total).with_easing(move |delta| after(waiting, delta)),
        move |element, delta| {
            element
                .opacity(delta)
                .relative()
                .top(px(distance * (1.0 - delta)))
        },
    )
}

/// Progress through an animation that waits for the first `waiting` part
/// of its time, then eases out over the rest.
fn after(waiting: f32, delta: f32) -> f32 {
    if delta <= waiting {
        return 0.0;
    }
    ease_out(((delta - waiting) / (1.0 - waiting)).min(1.0))
}

/// `element` fading in while it slides in from `distance` pixels to its
/// right, as a panel opening at the side does. Must not be absolute.
pub fn slide_in<E: IntoElement + Styled + 'static>(
    id: impl Into<ElementId>,
    element: E,
    distance: f32,
) -> AnimationElement<E> {
    element.with_animation(
        id,
        Animation::new(ENTER).with_easing(ease_out),
        move |element, delta| {
            element
                .opacity(delta)
                .relative()
                .left(px(distance * (1.0 - delta)))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{after, ease_out};

    #[test]
    fn a_breath_swells_and_falls_back() {
        use super::swell;
        assert!(swell(0.0).abs() < f32::EPSILON);
        assert!((swell(0.5) - 1.0).abs() < f32::EPSILON);
        assert!(swell(1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn eases_from_start_to_end() {
        assert!(ease_out(0.0).abs() < f32::EPSILON);
        assert!((ease_out(1.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn covers_most_of_the_way_early() {
        assert!(ease_out(0.5) > 0.8);
    }

    #[test]
    fn waits_before_moving() {
        assert!(after(0.5, 0.25).abs() < f32::EPSILON);
        assert!(after(0.5, 0.75) > 0.0);
        assert!((after(0.5, 1.0) - 1.0).abs() < f32::EPSILON);
    }
}
