//! A call floating over the main window as a small picture: the other
//! end's video, or their picture, with the user's own in its corner, so
//! chatting goes on all around it. It starts in the top right corner, clear
//! of the composer; it moves where it is dragged and grows or shrinks from
//! its bottom left corner. Clicking it docks the call back over the chat.

use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    Bounds, Context, Entity, EventEmitter, FontWeight, MouseButton, MouseDownEvent, Pixels, Point,
    Size, Window, div, point, px, rgb, rgba, size,
};
use natizap_profiles::ProfilePictures;
use natizap_ui::theme::{ActiveTheme as _, Theme};
use natizap_ui::{Button, ButtonSize, ButtonVariant, Icon, avatar};

use crate::bar::{CallPoppedOut, camera_button, mute_button};
use crate::desk::{CallDesk, CallLayout, CallShown, CameraState, Phase, VideoShown};
use crate::keys::HANG_UP;
use crate::look::ringed;
use crate::pointer::{Held, follower};
use crate::screen::Painted;

const CORNERS: f32 = 14.0;
/// The smallest the picture shrinks to: its controls still fit.
const LEAST: (f32, f32) = (220.0, 140.0);
/// How far the pointer goes before holding the panel is dragging it.
const DRAG_SLOP: f32 = 4.0;

/// The call under way, floating over the window while it is arranged so.
/// Draws nothing otherwise.
pub struct VideoPanel {
    desk: Entity<CallDesk>,
    pictures: Entity<ProfilePictures>,
    remote: Painted,
    preview: Painted,
    /// Where the user dragged the panel: its top left corner in the area it
    /// floats over. `None` until dragged.
    place: Option<Point<Pixels>>,
    /// The pointer holding the panel, while it does.
    held: Option<Hold>,
    /// The area the panel floats over, as last painted.
    area: Rc<Cell<Bounds<Pixels>>>,
}

/// The pointer holding the panel down.
#[derive(Debug, Clone, Copy)]
struct Hold {
    /// Where the pointer went down, in the window.
    from: Point<Pixels>,
    /// Whether it went far enough to be a drag rather than a click.
    moved: bool,
    grip: Grip,
}

/// What the pointer took hold of.
#[derive(Debug, Clone, Copy)]
enum Grip {
    /// The panel, this far into it.
    Body(Point<Pixels>),
    /// Its bottom left corner, with the panel's place and size when taken.
    Corner {
        place: Point<Pixels>,
        size: Size<Pixels>,
    },
}

impl std::fmt::Debug for VideoPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoPanel").finish_non_exhaustive()
    }
}

impl EventEmitter<CallPoppedOut> for VideoPanel {}

impl VideoPanel {
    /// Shows `desk`'s call while it floats.
    pub fn new(
        desk: Entity<CallDesk>,
        pictures: Entity<ProfilePictures>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&desk, |_, _, cx| cx.notify()).detach();
        cx.observe(&pictures, |_, _, cx| cx.notify()).detach();
        Self {
            desk,
            pictures,
            remote: Painted::default(),
            preview: Painted::default(),
            place: None,
            held: None,
            area: Rc::default(),
        }
    }

    /// Where the panel is, in its area, `size` big.
    fn place(&self, size: Size<Pixels>) -> Point<Pixels> {
        let area = self.area.get();
        let corner = point(area.size.width - size.width - px(16.0), px(16.0));
        inside(self.place.unwrap_or(corner), size, area)
    }

    fn hold(&mut self, at: Point<Pixels>, corner: bool, cx: &mut Context<Self>) {
        let size = self.desk.read(cx).floating_size();
        let place = self.place(size);
        let grip = if corner {
            Grip::Corner { place, size }
        } else {
            Grip::Body(at - self.area.get().origin - place)
        };
        self.held = Some(Hold {
            from: at,
            moved: false,
            grip,
        });
    }
}

impl Held for VideoPanel {
    fn moved(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(held) = self.held.as_mut() else {
            return;
        };
        let away = at - held.from;
        if !held.moved && f32::from(away.x).hypot(f32::from(away.y)) < DRAG_SLOP {
            return;
        }
        held.moved = true;
        let area = self.area.get();
        match held.grip {
            Grip::Body(grip) => {
                let size = self.desk.read(cx).floating_size();
                self.place = Some(inside(at - grip - area.origin, size, area));
            }
            Grip::Corner { place, size: taken } => {
                // The bottom left corner moves; the top right one stays.
                let right = place.x + taken.width;
                let left = (place.x + away.x).max(px(0.0));
                let grown = size(
                    (right - left).clamp(px(LEAST.0), right),
                    (taken.height + away.y).clamp(px(LEAST.1), area.size.height - place.y),
                );
                self.place = Some(point(right - grown.width, place.y));
                self.desk
                    .update(cx, |desk, cx| desk.set_floating_size(grown, cx));
            }
        }
        cx.notify();
    }

    fn released(&mut self, inside: bool, cx: &mut Context<Self>) {
        let held = self.held.take();
        // A press that never moved is a click: it docks the call again.
        if inside && held.is_some_and(|held| !held.moved && matches!(held.grip, Grip::Body(_))) {
            self.desk
                .update(cx, |desk, cx| desk.set_layout(CallLayout::Docked, cx));
        }
        cx.notify();
    }
}

/// `place` moved as little as needed for a panel `size` big to fit in
/// `area`.
fn inside(place: Point<Pixels>, size: Size<Pixels>, area: Bounds<Pixels>) -> Point<Pixels> {
    let most_x = (area.size.width - size.width).max(px(0.0));
    let most_y = (area.size.height - size.height).max(px(0.0));
    point(
        place.x.clamp(px(0.0), most_x),
        place.y.clamp(px(0.0), most_y),
    )
}

impl Render for VideoPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let desk = self.desk.read(cx);
        let floating = desk.layout() == CallLayout::Floating;
        let size = desk.floating_size();
        let call = desk
            .call(cx)
            .filter(|call| floating && !matches!(call.phase, Phase::Ended(_)));
        let Some(call) = call else {
            // Nothing painted now: let go of what was.
            self.remote.paint(None, px(0.0), window);
            self.preview.paint(None, px(0.0), window);
            return div().into_any_element();
        };
        let video = call.video.clone();
        let remote = self.remote.paint(
            video.as_ref().and_then(|video| video.remote.clone()),
            px(CORNERS),
            window,
        );
        let preview = self.preview.paint(
            video.as_ref().and_then(|video| video.preview.clone()),
            px(8.0),
            window,
        );
        let place = self.place(size);
        let showing = video.as_ref().is_some_and(|video| video.remote.is_some());
        let panel = div()
            .id("call-video-panel")
            .group("call-video-panel")
            .absolute()
            .left(place.x)
            .top(place.y)
            .w(size.width)
            .h(size.height)
            .occlude()
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|panel, event: &MouseDownEvent, _, cx| {
                    panel.hold(event.position, false, cx);
                    cx.stop_propagation();
                }),
            )
            .child(
                theme
                    .glass(div())
                    .size_full()
                    .relative()
                    .rounded(px(CORNERS))
                    .bg(rgb(theme.deep))
                    .child(remote)
                    .when(!showing, |stage| {
                        stage.child(self.camera_off(&call, video.is_some(), theme, cx))
                    })
                    .children(video.map(|video| {
                        div().absolute().bottom_2().right_2().child(own_picture(
                            &video,
                            preview,
                            theme,
                            px(96.0),
                            px(72.0),
                        ))
                    }))
                    .child(Self::controls(&call, cx))
                    .child(Self::corner(cx)),
            );
        div()
            .absolute()
            .inset_0()
            .child(follower(
                cx.weak_entity(),
                self.held.is_some(),
                Rc::clone(&self.area),
            ))
            .child(panel)
            .into_any_element()
    }
}

impl VideoPanel {
    /// The bottom left corner the panel is resized by.
    fn corner(cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .id("call-video-panel-corner")
            .absolute()
            .left_0()
            .bottom_0()
            .size(px(16.0))
            .cursor_nesw_resize()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|panel, event: &MouseDownEvent, _, cx| {
                    panel.hold(event.position, true, cx);
                    cx.stop_propagation();
                }),
            )
    }

    /// The call's controls along the panel's foot, shown while the pointer
    /// is over it. Their clicks stay theirs: a click elsewhere on the panel
    /// docks the call.
    fn controls(call: &CallShown, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .absolute()
            .bottom_2()
            .left_5()
            .flex()
            .items_center()
            .gap_1()
            .p_1()
            .rounded_full()
            .bg(rgba(cx.theme().scrim))
            .opacity(0.0)
            .group_hover("call-video-panel", |controls| controls.opacity(1.0))
            // A press on a control is the control's, not a hold of the panel.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(mute_button(
                call,
                ButtonSize::Small,
                cx.listener(|panel, _, _, cx| panel.desk.update(cx, CallDesk::toggle_muted)),
            ))
            .child(camera_button(
                call,
                ButtonSize::Small,
                cx.listener(|panel, _, _, cx| panel.desk.update(cx, CallDesk::toggle_camera)),
            ))
            .child(
                Button::new("call-panel-dock")
                    .icon(Icon::Maximize)
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Small)
                    .round()
                    .tooltip("Show over the chat")
                    .on_click(cx.listener(|panel, _, _, cx| {
                        panel
                            .desk
                            .update(cx, |desk, cx| desk.set_layout(CallLayout::Docked, cx));
                    })),
            )
            .child(
                Button::new("call-panel-pop-out")
                    .icon(Icon::PopOut)
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Small)
                    .round()
                    .tooltip("Open in its own window")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(CallPoppedOut))),
            )
            .child(
                Button::new("call-panel-hang-up")
                    .icon(Icon::CallEnd)
                    .variant(ButtonVariant::Danger)
                    .size(ButtonSize::Small)
                    .round()
                    .tooltip(HANG_UP.tip("Hang up"))
                    .on_click(cx.listener(|panel, _, _, cx| {
                        panel.desk.update(cx, CallDesk::hang_up);
                    })),
            )
    }

    /// Who the call is with, while their camera shows nothing.
    fn camera_off(
        &self,
        call: &CallShown,
        video: bool,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let picture = self
            .pictures
            .update(cx, |pictures, cx| pictures.picture(&call.peer, cx));
        div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .child(ringed(
                "call-panel-speaking",
                avatar(call.peer.as_str(), &call.title, picture, px(64.0)),
                64.0,
                call.speaking,
                theme,
            ))
            .child(
                div()
                    .text_size(px(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(theme.muted))
                    .child(if video {
                        "Camera off".into()
                    } else {
                        call.title.clone()
                    }),
            )
    }
}

/// The user's own picture, `width` by `height`, while the camera is on or
/// opening; the caller places it over the other end's.
pub(crate) fn own_picture(
    video: &VideoShown,
    painted: gpui::Div,
    theme: Theme,
    width: Pixels,
    height: Pixels,
) -> impl IntoElement {
    let shown = matches!(video.camera, CameraState::On | CameraState::Opening);
    div().when(shown, |corner| {
        corner.child(
            div()
                .relative()
                .w(width)
                .h(height)
                .rounded_lg()
                .border_1()
                .border_color(rgba(theme.edge))
                .bg(rgb(theme.panel))
                .child(painted),
        )
    })
}

#[cfg(test)]
mod tests {
    use gpui::{Bounds, point, px, size};

    use super::inside;

    #[test]
    fn a_dragged_panel_stays_inside_the_window() {
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
