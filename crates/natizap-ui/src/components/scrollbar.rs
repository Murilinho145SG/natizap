//! A thin scrollbar over the right edge of a scrolling list: it shows how
//! far the list has scrolled and can be dragged, or clicked to jump.

use std::cell::Cell;

use gpui::prelude::*;
use gpui::{
    App, Bounds, Canvas, ElementId, Entity, Hitbox, HitboxBehavior, ListState, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, ScrollHandle, UniformListScrollHandle,
    Window, canvas, point, px, quad, rgba, size,
};

use crate::theme::{THUMB, THUMB_ACTIVE};

/// Width the bar takes over the list's edge.
const WIDTH: f32 = 10.0;
/// Width of the thumb at rest, and under the pointer or while dragged.
const THIN: f32 = 4.0;
const WIDE: f32 = 7.0;
/// The thumb never gets shorter than this, however long the list.
const MIN_THUMB: f32 = 28.0;

/// The list a scrollbar follows.
#[derive(Clone)]
pub enum ScrollSource {
    /// A `list` of rows of any height.
    List(ListState),
    /// A `uniform_list`, or any element scrolled with a `ScrollHandle`.
    Handle(ScrollHandle),
}

impl std::fmt::Debug for ScrollSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::List(_) => "ScrollSource::List",
            Self::Handle(_) => "ScrollSource::Handle",
        })
    }
}

impl From<&ListState> for ScrollSource {
    fn from(list: &ListState) -> Self {
        Self::List(list.clone())
    }
}

impl From<&UniformListScrollHandle> for ScrollSource {
    fn from(handle: &UniformListScrollHandle) -> Self {
        Self::Handle(handle.0.borrow().base_handle.clone())
    }
}

impl From<&ScrollHandle> for ScrollSource {
    fn from(handle: &ScrollHandle) -> Self {
        Self::Handle(handle.clone())
    }
}

impl ScrollSource {
    /// How far the list has scrolled down, and how far it can.
    fn position(&self) -> (Pixels, Pixels) {
        match self {
            Self::List(list) => (
                -list.scroll_px_offset_for_scrollbar().y,
                list.max_offset_for_scrollbar().y,
            ),
            Self::Handle(handle) => (-handle.offset().y, handle.max_offset().y),
        }
    }

    /// How tall the visible part of the list is.
    fn viewport(&self) -> Pixels {
        match self {
            Self::List(list) => list.viewport_bounds().size.height,
            Self::Handle(handle) => handle.bounds().size.height,
        }
    }

    fn scroll_to(&self, offset: Pixels) {
        match self {
            Self::List(list) => list.set_offset_from_scrollbar(point(px(0.0), -offset)),
            Self::Handle(handle) => {
                let x = handle.offset().x;
                handle.set_offset(point(x, -offset));
            }
        }
    }

    /// A list keeps its measured height while dragged, so the thumb does
    /// not run from the pointer as rows get measured.
    fn dragging(&self, dragging: bool) {
        if let Self::List(list) = self {
            if dragging {
                list.scrollbar_drag_started();
            } else {
                list.scrollbar_drag_ended();
            }
        }
    }
}

/// Where the thumb is within `track`, for a list scrolled `offset` of
/// `max`, showing `viewport` of it at a time.
fn thumb_in(
    track: Bounds<Pixels>,
    offset: Pixels,
    max: Pixels,
    viewport: Pixels,
) -> (Pixels, Pixels) {
    let height = track.size.height;
    let thumb = (height * (viewport / (viewport + max)))
        .max(px(MIN_THUMB))
        .min(height);
    let progress = (offset / max).clamp(0.0, 1.0);
    (track.top() + (height - thumb) * progress, thumb)
}

/// A scrollbar for `source`, laid over the right edge of the element it is
/// a child of, which must be `relative()`. Hidden when everything fits.
pub fn scrollbar(
    id: impl Into<ElementId>,
    source: impl Into<ScrollSource>,
    window: &mut Window,
    cx: &mut App,
) -> Canvas<Hitbox> {
    let source: ScrollSource = source.into();
    // Where in the thumb it was grabbed, while it is dragged.
    let grab: Entity<Cell<Option<Pixels>>> =
        window.use_keyed_state(id.into(), cx, |_, _| Cell::new(None));
    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::BlockMouse),
        move |track, hitbox, window, cx| {
            let (offset, max) = source.position();
            let viewport = source.viewport();
            if max <= px(0.5) || viewport <= px(0.0) {
                return;
            }
            let (top, thumb) = thumb_in(track, offset, max, viewport);
            let dragged = grab.read(cx).get().is_some();
            let active = dragged || hitbox.is_hovered(window);
            let width = px(if active { WIDE } else { THIN });
            window.paint_quad(quad(
                Bounds::new(
                    point(track.right() - width - px(2.0), top),
                    size(width, thumb),
                ),
                width / 2.0,
                rgba(if active { THUMB_ACTIVE } else { THUMB }),
                px(0.0),
                gpui::transparent_black(),
                gpui::BorderStyle::default(),
            ));

            let pressing = (source.clone(), grab.clone(), hitbox.clone());
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                let (source, grab, hitbox) = &pressing;
                if !phase.bubble()
                    || event.button != MouseButton::Left
                    || !hitbox.is_hovered(window)
                {
                    return;
                }
                let y = event.position.y;
                // Grabbed where it was pressed; pressed off the thumb, it
                // jumps there, centered under the pointer.
                let held = if (top..top + thumb).contains(&y) {
                    y - top
                } else {
                    thumb / 2.0
                };
                grab.read(cx).set(Some(held));
                source.dragging(true);
                source.scroll_to(drag_offset(track, thumb, max, y - held));
                cx.stop_propagation();
                window.refresh();
            });
            let moving = (source.clone(), grab.clone());
            window.on_mouse_event(move |event: &MouseMoveEvent, _, window, cx| {
                let (source, grab) = &moving;
                let Some(held) = grab.read(cx).get() else {
                    return;
                };
                if event.pressed_button != Some(MouseButton::Left) {
                    return;
                }
                source.scroll_to(drag_offset(track, thumb, max, event.position.y - held));
                window.refresh();
            });
            let releasing = (source.clone(), grab.clone());
            window.on_mouse_event(move |_: &MouseUpEvent, _, window, cx| {
                let (source, grab) = &releasing;
                if grab.read(cx).take().is_some() {
                    source.dragging(false);
                    window.refresh();
                }
            });
        },
    )
    .absolute()
    .top_0()
    .right_0()
    .bottom_0()
    .w(px(WIDTH))
}

/// The offset that puts the thumb's top at `thumb_top`.
fn drag_offset(track: Bounds<Pixels>, thumb: Pixels, max: Pixels, thumb_top: Pixels) -> Pixels {
    let room = track.size.height - thumb;
    if room <= px(0.0) {
        return px(0.0);
    }
    max * ((thumb_top - track.top()) / room).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use gpui::{Bounds, point, px, size};

    use super::{drag_offset, thumb_in};

    fn track() -> Bounds<gpui::Pixels> {
        Bounds::new(point(px(0.0), px(100.0)), size(px(10.0), px(400.0)))
    }

    #[test]
    fn sizes_the_thumb_by_how_much_shows() {
        // A quarter of the list shows: a quarter of the track, at the top.
        assert_eq!(
            thumb_in(track(), px(0.0), px(1200.0), px(400.0)),
            (px(100.0), px(100.0))
        );
        // Scrolled to the end: at the bottom.
        assert_eq!(
            thumb_in(track(), px(1200.0), px(1200.0), px(400.0)),
            (px(400.0), px(100.0))
        );
        // A very long list still leaves a thumb to grab.
        assert_eq!(
            thumb_in(track(), px(0.0), px(400_000.0), px(400.0)).1,
            px(28.0)
        );
    }

    #[test]
    fn dragging_maps_the_thumb_back_to_an_offset() {
        assert_eq!(
            drag_offset(track(), px(100.0), px(1200.0), px(250.0)),
            px(600.0)
        );
        assert_eq!(
            drag_offset(track(), px(100.0), px(1200.0), px(0.0)),
            px(0.0)
        );
        assert_eq!(
            drag_offset(track(), px(100.0), px(1200.0), px(900.0)),
            px(1200.0)
        );
    }
}
