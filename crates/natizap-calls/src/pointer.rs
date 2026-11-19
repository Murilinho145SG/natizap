//! Following the pointer while it holds something of a call's view down:
//! the floating picture moved or resized, the docked call's edge dragged.
//! It is followed by hand, anywhere in the window, rather than as one of
//! GPUI's drags: those tell every view a drag is on, and the conversation
//! takes any drag for files and offers to drop them.

use gpui::{
    App, Bounds, Context, DispatchPhase, MouseButton, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    Size, WeakEntity, Window, canvas, div, point, prelude::*, px,
};

/// How far the pointer moves something it holds before it is dragged
/// instead of clicked.
pub(crate) const DRAG_SLOP: f32 = 4.0;

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

/// `place` moved as little as needed for something `size` big to fit in
/// `area`.
pub(crate) fn inside(
    place: Point<Pixels>,
    size: Size<Pixels>,
    area: Bounds<Pixels>,
) -> Point<Pixels> {
    let most_x = (area.size.width - size.width).max(px(0.0));
    let most_y = (area.size.height - size.height).max(px(0.0));
    point(
        place.x.clamp(px(0.0), most_x),
        place.y.clamp(px(0.0), most_y),
    )
}

#[cfg(test)]
mod tests {
    use gpui::{Bounds, point, px, size};

    use super::inside;

    #[test]
    fn something_dragged_stays_inside_the_window() {
        let area = Bounds::new(point(px(0.0), px(40.0)), size(px(1000.0), px(600.0)));
        let panel = size(px(340.0), px(220.0));
        // Within reach: it goes where it was put.
        assert_eq!(
            inside(point(px(100.0), px(50.0)), panel, area),
            point(px(100.0), px(50.0))
        );
        // Past the right and bottom edges, and above the top: held at them.
        assert_eq!(
            inside(point(px(990.0), px(-30.0)), panel, area),
            point(px(660.0), px(0.0))
        );
        assert_eq!(
            inside(point(px(-5.0), px(900.0)), panel, area),
            point(px(0.0), px(380.0))
        );
    }
}
