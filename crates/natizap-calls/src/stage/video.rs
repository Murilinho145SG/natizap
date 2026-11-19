//! The stage of a video call: its tiles, one focused and the others small
//! in a corner, or all side by side.

use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, MouseButton, MouseDownEvent, Stateful, Window, div, px, rgb, rgba,
};
use natizap_ui::theme::Theme;
use natizap_ui::{Button, ButtonVariant, Icon, avatar};

use super::{CallStage, Hold, Home};
use crate::desk::{CallShown, VideoShown};
use crate::screen::Painted;
use crate::tiles::{CONTROLS, GAP, Grab, SMALL, Tile, group_size};

/// What a stage painted of each tile's video.
#[derive(Default)]
pub(super) struct Paints {
    peer: Painted,
    camera: Painted,
    screen: Painted,
}

impl Paints {
    fn of(&mut self, tile: Tile) -> &mut Painted {
        match tile {
            Tile::Peer => &mut self.peer,
            Tile::Camera => &mut self.camera,
            Tile::Screen => &mut self.screen,
        }
    }

    /// Lets go of every picture painted, once no video shows.
    pub(super) fn clear(&mut self, window: &mut Window) {
        for tile in [Tile::Peer, Tile::Camera, Tile::Screen] {
            self.of(tile).paint(None, px(0.0), window);
        }
    }
}

/// How large a tile is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Size {
    /// Filling the stage.
    Focused,
    /// Side by side with the others.
    Grid,
    /// Small, in the corner.
    Small,
}

impl CallStage {
    /// The call with video: its tiles as the user arranged them.
    pub(super) fn pictured(
        &mut self,
        call: &CallShown,
        video: &VideoShown,
        theme: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let present = Tile::present(video);
        for tile in [Tile::Peer, Tile::Camera, Tile::Screen] {
            if !present.contains(&tile) {
                self.paints.of(tile).paint(None, px(0.0), window);
            }
        }
        let focused = self.arrangement.focused(&present);
        let small: Vec<Tile> = present
            .iter()
            .copied()
            .filter(|tile| focused.is_some_and(|focused| focused != *tile))
            .collect();
        self.small_count = small.len();
        let mut stage = div().size_full().relative().bg(rgb(theme.deep));
        if let Some(focused) = focused {
            stage = stage.child(self.tile(focused, Size::Focused, call, video, theme, window, cx));
            if !small.is_empty() {
                stage = stage.child(self.corner(&small, call, video, theme, window, cx));
            }
        } else {
            let tiles: Vec<_> = present
                .iter()
                .map(|tile| self.tile(*tile, Size::Grid, call, video, theme, window, cx))
                .collect();
            stage = stage.child(
                div()
                    .absolute()
                    .inset_0()
                    .pt(px(56.0))
                    .px(GAP)
                    .pb(CONTROLS)
                    .flex()
                    .gap(GAP)
                    .children(tiles),
            );
        }
        stage
            .when(present.len() > 1, |stage| {
                stage.child(self.view_switch(focused.is_some(), theme, cx))
            })
            .child(
                div()
                    .absolute()
                    .bottom_5()
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(Self::controls(call, cx)),
            )
            .into_any_element()
    }

    /// The small tiles, stacked in their corner; dragged, they follow the
    /// pointer until let go near another corner.
    fn corner(
        &mut self,
        small: &[Tile],
        call: &CallShown,
        video: &VideoShown,
        theme: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let group = group_size(small.len());
        let stage = self.area.get().size;
        let mut at = self.arrangement.corner.origin(stage, group);
        if let Some(Hold::Tile(Grab {
            moved: Some(by), ..
        })) = self.held
        {
            at += by;
        }
        let tiles: Vec<_> = small
            .iter()
            .map(|tile| self.tile(*tile, Size::Small, call, video, theme, window, cx))
            .collect();
        div()
            .absolute()
            .left(at.x)
            .top(at.y)
            .w(group.width)
            .flex()
            .flex_col()
            .gap(GAP)
            .children(tiles)
    }

    /// The button between one tile filling the stage and all side by side.
    fn view_switch(
        &self,
        focused: bool,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let button = if focused {
            Button::new("call-stage-grid")
                .icon(Icon::Grid)
                .tooltip("Show everyone side by side")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.arrangement.grid();
                    cx.notify();
                }))
        } else {
            Button::new("call-stage-focus")
                .icon(Icon::Maximize)
                .tooltip("Focus on them")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.arrangement.focus(Tile::Peer);
                    cx.notify();
                }))
        };
        // The main window's arrangement buttons take the top right; the
        // call's own window shows who is called at the top left.
        let place = div()
            .absolute()
            .top_2()
            .p_1()
            .rounded_full()
            .bg(rgba(theme.scrim));
        let place = if self.home == Home::Main {
            place.left_2()
        } else {
            place.right_2()
        };
        place.child(button.variant(ButtonVariant::Ghost).round())
    }

    /// One tile: its picture, or the other end's photo while theirs is
    /// off, and whose it is.
    #[expect(
        clippy::too_many_arguments,
        reason = "the tile, its size, the call and the drawing context are all it needs"
    )]
    fn tile(
        &mut self,
        tile: Tile,
        size: Size,
        call: &CallShown,
        video: &VideoShown,
        theme: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let picture = match tile {
            Tile::Peer => video.remote.clone(),
            Tile::Camera => video.camera_picture.clone(),
            Tile::Screen => video.screen_picture.clone(),
        };
        let corners = if size == Size::Focused {
            px(0.0)
        } else {
            px(10.0)
        };
        let missing = picture.is_none();
        let painted = self.paints.of(tile).paint(picture, corners, window);
        let base = div().id(tile.id()).relative().overflow_hidden();
        let base = match size {
            Size::Focused => base.absolute().inset_0(),
            Size::Grid => base
                .flex_1()
                .min_w_0()
                .h_full()
                .rounded(corners)
                .bg(rgb(theme.panel))
                .cursor_pointer()
                .hover(|tile| tile.border_color(rgba(theme.edge)))
                .border_1()
                .border_color(rgb(theme.line))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.arrangement.focus(tile);
                    cx.notify();
                })),
            Size::Small => base
                .w(SMALL.width)
                .h(SMALL.height)
                .flex_none()
                .rounded(corners)
                .bg(rgb(theme.panel))
                .border_1()
                .border_color(rgba(theme.edge))
                .shadow_lg()
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        this.held = Some(Hold::Tile(Grab::new(tile, event.position)));
                        cx.stop_propagation();
                        cx.notify();
                    }),
                ),
        };
        let base = base.child(painted);
        let base = if tile == Tile::Peer && missing {
            if size == Size::Focused {
                base.flex().flex_col().child(self.person(call, cx))
            } else {
                let picture = self
                    .pictures
                    .update(cx, |pictures, cx| pictures.picture(&call.peer, cx));
                let large = if size == Size::Grid { 96.0 } else { 48.0 };
                base.flex().items_center().justify_center().child(avatar(
                    call.peer.as_str(),
                    &call.title,
                    picture,
                    px(large),
                ))
            }
        } else {
            base
        };
        // The focused other end needs no label: the bar, or the call's
        // own window, says who it is.
        let label = match tile {
            Tile::Peer if size == Size::Focused => None,
            Tile::Peer => Some(call.title.clone()),
            Tile::Camera => Some("You".into()),
            Tile::Screen => Some("Your screen".into()),
        };
        base.when_some(label, |base, label| base.child(name_tag(label, theme)))
    }
}

/// Whose a tile is, at its bottom left.
fn name_tag(label: gpui::SharedString, theme: Theme) -> impl IntoElement {
    div()
        .absolute()
        .left_2()
        .bottom_2()
        .max_w(px(144.0))
        .truncate()
        .px_2()
        .py(px(2.0))
        .rounded_md()
        .bg(rgba(theme.scrim))
        .text_xs()
        .child(label)
}
