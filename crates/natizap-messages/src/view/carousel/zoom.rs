//! A photo in the carousel that zooms with the scroll wheel around the
//! pointer, moves with a drag once zoomed, and zooms in or back to fit
//! with a double click.

use gpui::prelude::*;
use gpui::{
    Context, CursorStyle, Div, ImageSource, MouseButton, MouseDownEvent, MouseMoveEvent, ObjectFit,
    Pixels, Point, ScrollEvent, Size, div, img, point, px,
};

use super::{SIDE, TOP_BAR, strip};
use crate::items::MediaView;
use crate::view::ConversationView;
use crate::zoom::{Vec2, Zoom};

/// Pixels the wheel moves for one notch.
const NOTCH_PIXELS: f32 = 40.0;

/// Where a photo shows: the room the stage leaves it, centred at `centre`
/// on the window, and the size at which it fits in it.
#[derive(Debug, Clone, Copy)]
pub(super) struct Geometry {
    pub(super) fit: Vec2,
    room: Vec2,
    centre: Point<Pixels>,
}

impl Geometry {
    /// `media`'s photo in a window `viewport` big.
    pub(super) fn of(media: &MediaView, viewport: Size<Pixels>) -> Self {
        let stage_h = f32::from(viewport.height) - TOP_BAR - strip::HEIGHT;
        let room = Vec2::new(f32::from(viewport.width) - 2.0 * SIDE, stage_h - 32.0);
        // A size the message did not carry takes the whole room.
        let (width, height) = match media.size {
            (width, height) if width > 0.0 && height > 0.0 => (width, height),
            _ => (room.x, room.y),
        };
        let scale = (room.x / width).min(room.y / height).max(0.0);
        Self {
            fit: Vec2::new(width * scale, height * scale),
            room,
            centre: point(viewport.width / 2.0, px(TOP_BAR + stage_h / 2.0)),
        }
    }

    /// `position` on the window, from the room's centre.
    fn off_centre(&self, position: Point<Pixels>) -> Vec2 {
        Vec2::new(
            f32::from(position.x - self.centre.x),
            f32::from(position.y - self.centre.y),
        )
    }

    /// Whether `position` falls on the photo as `zoom` shows it.
    fn on_photo(&self, zoom: Zoom, position: Point<Pixels>) -> bool {
        let (origin, size) = zoom.placed(self.fit, self.room);
        let at = self.off_centre(position);
        let (x, y) = (at.x + self.room.x / 2.0, at.y + self.room.y / 2.0);
        (origin.x..origin.x + size.x).contains(&x) && (origin.y..origin.y + size.y).contains(&y)
    }
}

/// The photo from `source`, zoomed as `zoom` says, in its room.
pub(super) fn photo(
    source: ImageSource,
    zoom: Zoom,
    geometry: Geometry,
    cx: &mut Context<ConversationView>,
) -> Div {
    let (origin, size) = zoom.placed(geometry.fit, geometry.room);
    div()
        .relative()
        .w(px(geometry.room.x))
        .h(px(geometry.room.y))
        .overflow_hidden()
        .when(zoom.zoomed(), |room| room.cursor(CursorStyle::OpenHand))
        .on_scroll(cx.listener(move |this, event: &ScrollEvent, _, cx| {
            let notches = f32::from(event.delta.pixel_delta(px(NOTCH_PIXELS)).y) / NOTCH_PIXELS;
            let pointer = geometry.off_centre(event.position);
            update(this, cx, |zoom| {
                zoom.wheel(notches, pointer, geometry.fit, geometry.room)
            });
            cx.stop_propagation();
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                let Some(carousel) = this.carousel.as_mut() else {
                    return;
                };
                // Beside the photo at fit size, the click goes on to close
                // the carousel, as before it zoomed.
                if !carousel.zoom.zoomed() && !geometry.on_photo(carousel.zoom, event.position) {
                    return;
                }
                cx.stop_propagation();
                if event.click_count == 2 {
                    let pointer = geometry.off_centre(event.position);
                    carousel.zoom = carousel.zoom.toggle(pointer, geometry.fit, geometry.room);
                    carousel.dragging = None;
                } else if carousel.zoom.zoomed() {
                    carousel.dragging = Some(event.position);
                }
                cx.notify();
            }),
        )
        .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
            let Some(carousel) = this.carousel.as_mut() else {
                return;
            };
            let Some(from) = carousel.dragging else {
                return;
            };
            if event.pressed_button != Some(MouseButton::Left) {
                carousel.dragging = None;
                return;
            }
            let delta = Vec2::new(
                f32::from(event.position.x - from.x),
                f32::from(event.position.y - from.y),
            );
            carousel.dragging = Some(event.position);
            carousel.zoom = carousel.zoom.drag(delta, geometry.fit, geometry.room);
            cx.notify();
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _, _, _| {
                if let Some(carousel) = this.carousel.as_mut() {
                    carousel.dragging = None;
                }
            }),
        )
        .child(
            img(source)
                .absolute()
                .left(px(origin.x))
                .top(px(origin.y))
                .w(px(size.x))
                .h(px(size.y))
                .rounded(px(8.0))
                .object_fit(ObjectFit::Contain),
        )
}

fn update(
    view: &mut ConversationView,
    cx: &mut Context<ConversationView>,
    f: impl FnOnce(Zoom) -> Zoom,
) {
    if let Some(carousel) = view.carousel.as_mut() {
        carousel.zoom = f(carousel.zoom);
        cx.notify();
    }
}
