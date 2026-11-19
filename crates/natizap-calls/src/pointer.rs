//! Following the pointer while it holds something of a call's view down:
//! the floating picture moved or resized, the docked call's edge dragged.
//! It is followed by hand, anywhere in the window, rather than as one of
//! GPUI's drags: those tell every view a drag is on, and the conversation
//! takes any drag for files and offers to drop them.

use gpui::{
    App, Bounds, Context, DispatchPhase, MouseButton, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    WeakEntity, Window, canvas, div, prelude::*,
};

/// What a view does as the pointer holding it moves, and when it lets go.
pub(crate) trait Held: 'static {
    /// The pointer, still down, is at `at`, in the window.
    fn moved(&mut self, at: Point<Pixels>, cx: &mut Context<Self>)
    where
        Self: Sized;
    /// The pointer let go: `inside` when it did so in the window, where its
    /// release is heard; outside, the next move finds no button down.
    fn released(&mut self, inside: bool, cx: &mut Context<Self>)
    where
        Self: Sized;
}

/// An element filling its parent that, while `holding`, follows the
/// pointer for `view` everywhere in the window. It measures the parent as
/// it paints, into `area`, for the view to keep what it moves inside.
pub(crate) fn follower<T: Held>(
    view: WeakEntity<T>,
    holding: bool,
    area: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
) -> impl IntoElement {
    div().absolute().inset_0().child(
        canvas(
            move |bounds, _, _| area.set(bounds),
            move |_, (), window, _| {
                if holding {
                    listen(&view, window);
                }
            },
        )
        .absolute()
        .size_full(),
    )
}

fn listen<T: Held>(view: &WeakEntity<T>, window: &mut Window) {
    let moving = view.clone();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx: &mut App| {
        if phase != DispatchPhase::Bubble {
            return;
        }
        let _ = moving.update(cx, |view, cx| {
            if event.pressed_button == Some(MouseButton::Left) {
                view.moved(event.position, cx);
            } else {
                view.released(false, cx);
            }
        });
    });
    let letting = view.clone();
    window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx: &mut App| {
        if phase == DispatchPhase::Bubble {
            let _ = letting.update(cx, |view, cx| view.released(true, cx));
        }
    });
}
