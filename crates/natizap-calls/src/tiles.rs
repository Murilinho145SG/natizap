//! How a video call's pictures share the stage, as on Discord: each one is
//! a tile (the other end, the user's camera, the user's shared screen).
//! One tile is focused and fills the stage while the others float small
//! in a corner the user drags them to, or all of them sit side by side
//! in a grid. Clicking a tile focuses it.

use gpui::{Bounds, Pixels, Point, Size, point, px, size};

use crate::desk::{CameraState, Sharing, VideoShown};

/// A small tile's size, beside the focused one.
pub(crate) const SMALL: Size<Pixels> = Size {
    width: px(176.0),
    height: px(110.0),
};
/// The gap between small tiles, and between them and the stage's edges.
pub(crate) const GAP: Pixels = px(12.0);
/// What the controls along the stage's foot take, kept clear of tiles.
pub(crate) const CONTROLS: Pixels = px(84.0);
/// How far the pointer moves a small tile before it is dragged instead
/// of clicked.
const DRAG_SLOP: f32 = 4.0;

/// One picture of a video call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Tile {
    /// The other end: their camera, or their picture while it is off.
    Peer,
    /// The user's camera.
    Camera,
    /// The user's shared screen.
    Screen,
}

impl Tile {
    /// The tiles `video` has now, the other end's first.
    pub(crate) fn present(video: &VideoShown) -> Vec<Self> {
        let mut tiles = vec![Self::Peer];
        if matches!(video.camera, CameraState::On | CameraState::Opening) {
            tiles.push(Self::Camera);
        }
        if video.sharing == Sharing::On {
            tiles.push(Self::Screen);
        }
        tiles
    }

    /// The element id of the tile, unique within a stage.
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Peer => "call-tile-peer",
            Self::Camera => "call-tile-camera",
            Self::Screen => "call-tile-screen",
        }
    }
}

/// A corner of the stage the small tiles float in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    /// The corner of `area` nearest to `at`.
    pub(crate) fn nearest(area: Bounds<Pixels>, at: Point<Pixels>) -> Self {
        let middle = area.center();
        match (at.x < middle.x, at.y < middle.y) {
            (true, true) => Self::TopLeft,
            (false, true) => Self::TopRight,
            (true, false) => Self::BottomLeft,
            (false, false) => Self::BottomRight,
        }
    }

    /// Where a group of small tiles `group` large sits in this corner of a
    /// stage `stage` large, from the stage's top left: clear of its edges,
    /// and of the controls along its foot.
    pub(crate) fn origin(self, stage: Size<Pixels>, group: Size<Pixels>) -> Point<Pixels> {
        let left = GAP;
        let right = (stage.width - group.width - GAP).max(GAP);
        // The top keeps clear of the stage's own buttons there.
        let top = GAP + px(44.0);
        let bottom = (stage.height - group.height - CONTROLS).max(top);
        match self {
            Self::TopLeft => point(left, top),
            Self::TopRight => point(right, top),
            Self::BottomLeft => point(left, bottom),
            Self::BottomRight => point(right, bottom),
        }
    }
}

/// The size of `count` small tiles stacked in their corner.
pub(crate) fn group_size(count: usize) -> Size<Pixels> {
    let count = count.max(1);
    #[expect(
        clippy::cast_precision_loss,
        reason = "a call has at most a handful of tiles"
    )]
    let (tiles, gaps) = (count as f32, (count - 1) as f32);
    size(SMALL.width, SMALL.height * tiles + GAP * gaps)
}

/// How the user arranged a call's tiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Arrangement {
    /// The tile filling the stage, or none for the grid.
    focus: Option<Tile>,
    /// Where the small tiles float.
    pub(crate) corner: Corner,
}

impl Default for Arrangement {
    fn default() -> Self {
        Self {
            focus: Some(Tile::Peer),
            corner: Corner::BottomRight,
        }
    }
}

impl Arrangement {
    /// The tile filling the stage among `present`, or none for the grid. A
    /// focused tile that went away (the screen no longer shared) gives way
    /// to the other end's.
    pub(crate) fn focused(self, present: &[Tile]) -> Option<Tile> {
        let focus = self.focus?;
        Some(if present.contains(&focus) {
            focus
        } else {
            Tile::Peer
        })
    }

    /// Fills the stage with `tile`.
    pub(crate) fn focus(&mut self, tile: Tile) {
        self.focus = Some(tile);
    }

    /// Shows every tile side by side.
    pub(crate) fn grid(&mut self) {
        self.focus = None;
    }
}

/// A small tile held down by the pointer: clicked, or dragged with the
/// others to another corner.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Grab {
    pub(crate) tile: Tile,
    from: Point<Pixels>,
    /// How far the pointer moved the tiles, while they are dragged.
    pub(crate) moved: Option<Point<Pixels>>,
}

impl Grab {
    pub(crate) fn new(tile: Tile, at: Point<Pixels>) -> Self {
        Self {
            tile,
            from: at,
            moved: None,
        }
    }

    /// The pointer, still down, is at `at`.
    pub(crate) fn follow(&mut self, at: Point<Pixels>) {
        let by = at - self.from;
        let far = by.x.abs() > px(DRAG_SLOP) || by.y.abs() > px(DRAG_SLOP);
        if far || self.moved.is_some() {
            self.moved = Some(by);
        }
    }

    /// The pointer let go: the corner the tiles were dragged nearest to,
    /// or none when it was a click. `stage` is the stage in the window.
    pub(crate) fn dropped(
        &self,
        stage: Bounds<Pixels>,
        from: Corner,
        count: usize,
    ) -> Option<Corner> {
        let by = self.moved?;
        let group = group_size(count);
        let origin = stage.origin + from.origin(stage.size, group) + by;
        let middle = origin + point(group.width / 2.0, group.height / 2.0);
        Some(Corner::nearest(stage, middle))
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Bounds, point, px, size};

    use super::{Arrangement, Corner, Grab, Tile};

    #[test]
    fn a_focused_tile_that_goes_away_gives_way_to_the_other_end() {
        let mut arrangement = Arrangement::default();
        arrangement.focus(Tile::Screen);
        let sharing = [Tile::Peer, Tile::Camera, Tile::Screen];
        assert_eq!(arrangement.focused(&sharing), Some(Tile::Screen));
        assert_eq!(
            arrangement.focused(&[Tile::Peer, Tile::Camera]),
            Some(Tile::Peer)
        );
        arrangement.grid();
        assert_eq!(arrangement.focused(&sharing), None);
    }

    #[test]
    fn dragged_tiles_land_in_the_nearest_corner() {
        let stage = Bounds::new(point(px(100.0), px(50.0)), size(px(1000.0), px(600.0)));
        let mut grab = Grab::new(Tile::Camera, point(px(900.0), px(500.0)));
        // A small move is a click.
        grab.follow(point(px(902.0), px(501.0)));
        assert_eq!(grab.dropped(stage, Corner::BottomRight, 1), None);
        // Across to the left, and up past the middle.
        grab.follow(point(px(200.0), px(150.0)));
        assert_eq!(
            grab.dropped(stage, Corner::BottomRight, 1),
            Some(Corner::TopLeft)
        );
        // Back near where it came from.
        grab.follow(point(px(880.0), px(480.0)));
        assert_eq!(
            grab.dropped(stage, Corner::BottomRight, 1),
            Some(Corner::BottomRight)
        );
    }

    #[test]
    fn tiles_keep_clear_of_the_controls() {
        let stage = size(px(800.0), px(500.0));
        let group = super::group_size(2);
        let origin = Corner::BottomLeft.origin(stage, group);
        assert!(origin.y + group.height <= stage.height - super::CONTROLS + px(0.5));
        assert_eq!(origin.x, super::GAP);
    }
}
