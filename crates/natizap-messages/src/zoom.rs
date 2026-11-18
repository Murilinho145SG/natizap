//! How far a photo is zoomed and where it is moved to, as the scroll wheel,
//! a drag or a double click change it. Plain numbers in pixels, so the
//! geometry is tested without a window.

/// A point or a size in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Vec2 {
    pub(crate) x: f32,
    pub(crate) y: f32,
}

impl Vec2 {
    pub(crate) fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// The closest and farthest a photo zooms.
const LEAST: f32 = 1.0;
const MOST: f32 = 8.0;
/// How much one notch of the wheel zooms.
const NOTCH: f32 = 1.15;
/// Where a double click zooms to.
const DOUBLE_CLICK: f32 = 2.5;

/// A photo's zoom: `scale` times the size that fits, its centre moved by
/// `pan` from the centre of the room it shows in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Zoom {
    pub(crate) scale: f32,
    pub(crate) pan: Vec2,
}

impl Default for Zoom {
    fn default() -> Self {
        Self {
            scale: LEAST,
            pan: Vec2::default(),
        }
    }
}

impl Zoom {
    /// Whether the photo is bigger than it fits.
    pub(crate) fn zoomed(&self) -> bool {
        self.scale > LEAST
    }

    /// Zoomed by `notches` of the wheel (in when positive), keeping the
    /// spot under `pointer` (from the room's centre) under it.
    pub(crate) fn wheel(self, notches: f32, pointer: Vec2, fit: Vec2, room: Vec2) -> Self {
        self.zoom_to(self.scale * NOTCH.powf(notches), pointer, fit, room)
    }

    /// A double click: in to a closer look at `pointer`, or back to fit.
    pub(crate) fn toggle(self, pointer: Vec2, fit: Vec2, room: Vec2) -> Self {
        if self.zoomed() {
            Self::default()
        } else {
            self.zoom_to(DOUBLE_CLICK, pointer, fit, room)
        }
    }

    /// Moved by `delta`, as a drag does.
    pub(crate) fn drag(self, delta: Vec2, fit: Vec2, room: Vec2) -> Self {
        Self {
            pan: Vec2::new(self.pan.x + delta.x, self.pan.y + delta.y),
            ..self
        }
        .held(fit, room)
    }

    fn zoom_to(self, scale: f32, pointer: Vec2, fit: Vec2, room: Vec2) -> Self {
        let scale = scale.clamp(LEAST, MOST);
        // The spot under the pointer sits at `(pointer - pan) / scale` on
        // the photo; keep it there at the new scale.
        let ratio = scale / self.scale;
        Self {
            scale,
            pan: Vec2::new(
                pointer.x - (pointer.x - self.pan.x) * ratio,
                pointer.y - (pointer.y - self.pan.y) * ratio,
            ),
        }
        .held(fit, room)
    }

    /// Kept from showing empty room beside the photo: centred along an
    /// axis it does not fill, held to its edges along one it overflows.
    fn held(self, fit: Vec2, room: Vec2) -> Self {
        let hold = |pan: f32, shown: f32, room: f32| {
            let slack = ((shown - room) / 2.0).max(0.0);
            pan.clamp(-slack, slack)
        };
        Self {
            pan: Vec2::new(
                hold(self.pan.x, fit.x * self.scale, room.x),
                hold(self.pan.y, fit.y * self.scale, room.y),
            ),
            ..self
        }
    }

    /// Where the photo's top left corner goes in the room, and its size.
    pub(crate) fn placed(&self, fit: Vec2, room: Vec2) -> (Vec2, Vec2) {
        let shown = Vec2::new(fit.x * self.scale, fit.y * self.scale);
        let origin = Vec2::new(
            (room.x - shown.x) / 2.0 + self.pan.x,
            (room.y - shown.y) / 2.0 + self.pan.y,
        );
        (origin, shown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIT: Vec2 = Vec2 { x: 400.0, y: 300.0 };
    const ROOM: Vec2 = Vec2 { x: 800.0, y: 600.0 };

    fn close(a: Vec2, b: Vec2) -> bool {
        (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01
    }

    #[test]
    fn keeps_the_spot_under_the_pointer_still() {
        let pointer = Vec2::new(150.0, -100.0);
        let start = Zoom {
            scale: 2.0,
            pan: Vec2::default(),
        };
        // The photo's spot under the pointer, in fit pixels, before and
        // after.
        let spot = |zoom: Zoom| {
            Vec2::new(
                (pointer.x - zoom.pan.x) / zoom.scale,
                (pointer.y - zoom.pan.y) / zoom.scale,
            )
        };
        let zoomed = start.wheel(3.0, pointer, FIT, ROOM);
        assert!(zoomed.scale > start.scale);
        assert!(close(spot(start), spot(zoomed)));
    }

    #[test]
    fn stays_between_fit_and_its_closest() {
        let out = Zoom::default().wheel(-10.0, Vec2::default(), FIT, ROOM);
        assert_eq!(out, Zoom::default());
        let deep = Zoom::default().wheel(100.0, Vec2::default(), FIT, ROOM);
        assert!((deep.scale - MOST).abs() < f32::EPSILON);
    }

    #[test]
    fn never_drags_the_photo_off_its_room() {
        let zoomed = Zoom {
            scale: 4.0,
            pan: Vec2::default(),
        };
        // 1600 × 1200 shown in an 800 × 600 room: 400 and 300 of slack.
        let dragged = zoomed.drag(Vec2::new(10_000.0, -10_000.0), FIT, ROOM);
        assert!(close(dragged.pan, Vec2::new(400.0, -300.0)));
        // At fit size it fills less than the room, so it stays centred.
        let fit = Zoom::default().drag(Vec2::new(50.0, 50.0), FIT, ROOM);
        assert!(close(fit.pan, Vec2::default()));
    }

    #[test]
    fn a_double_click_zooms_in_then_back_to_fit() {
        let pointer = Vec2::new(100.0, 50.0);
        let near = Zoom::default().toggle(pointer, FIT, ROOM);
        assert!(near.zoomed());
        assert_eq!(near.toggle(pointer, FIT, ROOM), Zoom::default());
    }

    #[test]
    fn places_the_photo_centred_then_moved() {
        let zoom = Zoom {
            scale: 2.0,
            pan: Vec2::new(30.0, -20.0),
        };
        let (origin, size) = zoom.placed(FIT, ROOM);
        assert!(close(size, Vec2::new(800.0, 600.0)));
        assert!(close(origin, Vec2::new(30.0, -20.0)));
    }
}
