//! A video call inside the main window: a small floating picture, the
//! other end's video with the user's own in its corner, so chatting goes on
//! beside it. It starts in the top right corner, clear of the composer,
//! and can be dragged anywhere in the window; clicking it moves the call to
//! a window of its own, where the video is large.

use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    Bounds, Context, DragMoveEvent, Entity, EventEmitter, FontWeight, Pixels, Point, Window,
    canvas, div, point, px, rgb, rgba,
};
use natizap_profiles::ProfilePictures;
use natizap_ui::theme::{ActiveTheme as _, Theme};
use natizap_ui::{Button, ButtonSize, ButtonVariant, Icon, avatar};

use crate::bar::{CallPoppedOut, camera_button, mute_button};
use crate::desk::{CallDesk, CallShown, CameraState, Phase, VideoShown};
use crate::keys::HANG_UP;
use crate::look::ringed;
use crate::screen::Painted;

const WIDTH: f32 = 340.0;
const HEIGHT: f32 = 220.0;
const CORNERS: f32 = 14.0;

/// The video of the call under way, floating over the window. Draws
/// nothing without video.
pub struct VideoPanel {
    desk: Entity<CallDesk>,
    pictures: Entity<ProfilePictures>,
    remote: Painted,
    preview: Painted,
    /// Where the user dragged the panel: its top left corner in the area it
    /// floats over. `None` until dragged.
    place: Option<Point<Pixels>>,
    /// Where in the panel the pointer took hold of it.
    grip: Point<Pixels>,
    /// The area the panel floats over, as last painted.
    area: Rc<Cell<Bounds<Pixels>>>,
}

/// The panel being dragged.
struct Dragged;

/// What a drag shows under the pointer: nothing, the panel itself moves.
struct NoGhost;

impl Render for NoGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl std::fmt::Debug for VideoPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoPanel").finish_non_exhaustive()
    }
}

impl EventEmitter<CallPoppedOut> for VideoPanel {}

impl VideoPanel {
    /// Shows `desk`'s call video.
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
            grip: Point::default(),
            area: Rc::default(),
        }
    }

    /// Follows the pointer dragging the panel, keeping it inside its area.
    fn drag(&mut self, event: &DragMoveEvent<Dragged>, cx: &mut Context<Self>) {
        let area = self.area.get();
        let wanted = event.event.position - self.grip - area.origin;
        self.place = Some(inside(wanted, area));
        cx.notify();
    }
}

/// `place` moved as little as needed for the panel to fit in `area`.
fn inside(place: Point<Pixels>, area: Bounds<Pixels>) -> Point<Pixels> {
    let most_x = (area.size.width - px(WIDTH)).max(px(0.0));
    let most_y = (area.size.height - px(HEIGHT)).max(px(0.0));
    point(
        place.x.clamp(px(0.0), most_x),
        place.y.clamp(px(0.0), most_y),
    )
}

impl Render for VideoPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let call = self.desk.read(cx).call(cx);
        let Some((call, video)) = call
            .filter(|call| !matches!(call.phase, Phase::Ended(_)))
            .and_then(|call| call.video.clone().map(|video| (call, video)))
        else {
            // Nothing painted now: let go of what was.
            self.remote.paint(None, px(0.0), window);
            self.preview.paint(None, px(0.0), window);
            return div().into_any_element();
        };
        let remote = self.remote.paint(video.remote.clone(), px(CORNERS), window);
        let preview = self.preview.paint(video.preview.clone(), px(8.0), window);
        // The window may have shrunk since the panel was put somewhere.
        let place = self.place.map(|place| inside(place, self.area.get()));
        let measured = Rc::clone(&self.area);
        let grip = cx.weak_entity();
        let panel = div()
            .id("call-video-panel")
            .group("call-video-panel")
            .absolute()
            .map(|panel| match place {
                Some(place) => panel.left(place.x).top(place.y),
                None => panel.top_4().right_4(),
            })
            .w(px(WIDTH))
            .h(px(HEIGHT))
            .occlude()
            .cursor_pointer()
            .on_click(cx.listener(|_, _, _, cx| cx.emit(CallPoppedOut)))
            .on_drag(Dragged, move |_, offset, _, cx| {
                let _ = grip.update(cx, |panel, _| panel.grip = offset);
                cx.new(|_| NoGhost)
            })
            .on_drag_move(cx.listener(|panel, event: &DragMoveEvent<Dragged>, _, cx| {
                panel.drag(event, cx);
            }))
            .child(
                theme
                    .glass(div())
                    .size_full()
                    .relative()
                    .rounded(px(CORNERS))
                    .bg(rgb(theme.deep))
                    .child(remote)
                    .when(video.remote.is_none(), |stage| {
                        stage.child(self.camera_off(&call, theme, cx))
                    })
                    .child(div().absolute().bottom_2().right_2().child(own_picture(
                        &video,
                        preview,
                        theme,
                        px(96.0),
                        px(72.0),
                    )))
                    .child(Self::controls(&call, cx)),
            );
        // The area the panel floats over, measured as it is painted: the
        // panel's drag is kept inside it.
        div()
            .absolute()
            .inset_0()
            .child(
                canvas(move |bounds, _, _| measured.set(bounds), |_, (), _, _| {})
                    .absolute()
                    .size_full(),
            )
            .child(panel)
            .into_any_element()
    }
}

impl VideoPanel {
    /// The call's controls along the panel's foot, shown while the pointer
    /// is over it. Their clicks stay theirs: a click elsewhere on the panel
    /// moves the call to its own window.
    fn controls(call: &CallShown, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .absolute()
            .bottom_2()
            .left_2()
            .flex()
            .items_center()
            .gap_1()
            .p_1()
            .rounded_full()
            .bg(rgba(cx.theme().scrim))
            .opacity(0.0)
            .group_hover("call-video-panel", |controls| controls.opacity(1.0))
            .child(mute_button(
                call,
                ButtonSize::Small,
                cx.listener(|panel, _, _, cx| {
                    cx.stop_propagation();
                    panel.desk.update(cx, CallDesk::toggle_muted);
                }),
            ))
            .child(camera_button(
                call,
                ButtonSize::Small,
                cx.listener(|panel, _, _, cx| {
                    cx.stop_propagation();
                    panel.desk.update(cx, CallDesk::toggle_camera);
                }),
            ))
            .child(
                Button::new("call-panel-hang-up")
                    .icon(Icon::CallEnd)
                    .variant(ButtonVariant::Danger)
                    .size(ButtonSize::Small)
                    .round()
                    .tooltip(HANG_UP.tip("Hang up"))
                    .on_click(cx.listener(|panel, _, _, cx| {
                        cx.stop_propagation();
                        panel.desk.update(cx, CallDesk::hang_up);
                    })),
            )
    }

    /// Who the call is with, while their camera shows nothing.
    fn camera_off(
        &self,
        call: &CallShown,
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
                    .child("Camera off"),
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

    use super::{HEIGHT, WIDTH, inside};

    #[test]
    fn a_dragged_panel_stays_inside_the_window() {
        let area = Bounds::new(point(px(0.0), px(40.0)), size(px(1000.0), px(600.0)));
        // Within reach: it goes where it was put.
        assert_eq!(
            inside(point(px(100.0), px(50.0)), area),
            point(px(100.0), px(50.0))
        );
        // Past the right and bottom edges, and above the top: held at them.
        assert_eq!(
            inside(point(px(990.0), px(-30.0)), area),
            point(px(1000.0 - WIDTH), px(0.0))
        );
        assert_eq!(
            inside(point(px(-5.0), px(900.0)), area),
            point(px(0.0), px(600.0 - HEIGHT))
        );
    }
}
