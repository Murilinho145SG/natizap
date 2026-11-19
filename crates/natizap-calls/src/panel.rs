//! A video call inside the main window: a small floating picture, the
//! other end's video with the user's own in its corner, so chatting goes on
//! beside it. It starts in the top right corner, clear of the composer,
//! and can be dragged anywhere in the window; clicking it moves the call to
//! a window of its own, where the video is large.

use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    Bounds, Context, DispatchPhase, Entity, EventEmitter, FontWeight, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, Window, canvas, div, point, px, rgb, rgba,
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
    /// The pointer holding the panel, while it does.
    held: Option<Hold>,
    /// The area the panel floats over, as last painted.
    area: Rc<Cell<Bounds<Pixels>>>,
}

/// The pointer holding the panel down. The panel follows it by hand,
/// rather than as one of GPUI's drags: those tell every view a drag is on,
/// and the conversation takes any drag for files and offers to drop them.
#[derive(Debug, Clone, Copy)]
struct Hold {
    /// Where in the panel the pointer took hold of it.
    grip: Point<Pixels>,
    /// Where the pointer went down, in the window.
    from: Point<Pixels>,
    /// Whether it went far enough to be a drag rather than a click.
    moved: bool,
}

/// How far the pointer goes before holding the panel is dragging it.
const DRAG_SLOP: f32 = 4.0;

/// The panel's default place: the area's top right corner, clear of the
/// composer at the bottom.
fn corner(area: Bounds<Pixels>) -> Point<Pixels> {
    point(area.size.width - px(WIDTH + 16.0), px(16.0))
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
            held: None,
            area: Rc::default(),
        }
    }

    /// Where the panel is, in its area.
    fn place(&self) -> Point<Pixels> {
        let area = self.area.get();
        inside(self.place.unwrap_or_else(|| corner(area)), area)
    }

    /// The pointer went down on the panel at `at`, in the window.
    fn hold(&mut self, at: Point<Pixels>) {
        let origin = self.area.get().origin + self.place();
        self.held = Some(Hold {
            grip: at - origin,
            from: at,
            moved: false,
        });
    }

    /// The pointer holding the panel moved to `at`: past the slop, the
    /// panel follows it, kept inside its area.
    fn follow(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(held) = self.held.as_mut() else {
            return;
        };
        let away = at - held.from;
        if !held.moved && f32::from(away.x).hypot(f32::from(away.y)) < DRAG_SLOP {
            return;
        }
        held.moved = true;
        let area = self.area.get();
        self.place = Some(inside(at - held.grip - area.origin, area));
        cx.notify();
    }

    /// The pointer let go: a hold that never moved was a click, which opens
    /// the call's own window.
    fn release(&mut self, cx: &mut Context<Self>) {
        if self.held.take().is_some_and(|held| !held.moved) {
            cx.emit(CallPoppedOut);
        }
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
        let place = self.place();
        let measured = Rc::clone(&self.area);
        let holding = self.held.is_some();
        let panel_entity = cx.weak_entity();
        let panel = div()
            .id("call-video-panel")
            .group("call-video-panel")
            .absolute()
            .left(place.x)
            .top(place.y)
            .w(px(WIDTH))
            .h(px(HEIGHT))
            .occlude()
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|panel, event: &MouseDownEvent, _, cx| {
                    panel.hold(event.position);
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
                canvas(
                    move |bounds, _, _| measured.set(bounds),
                    move |_, (), window, _| {
                        if !holding {
                            return;
                        }
                        // While held, the pointer is followed wherever it
                        // goes in the window, off the panel too.
                        let moving = panel_entity.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase == DispatchPhase::Bubble {
                                let _ = moving.update(cx, |panel, cx| {
                                    // Let go outside the window, where its
                                    // release is never heard.
                                    if event.pressed_button == Some(MouseButton::Left) {
                                        panel.follow(event.position, cx);
                                    } else {
                                        panel.held = None;
                                        cx.notify();
                                    }
                                });
                            }
                        });
                        let letting = panel_entity.clone();
                        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                            if phase == DispatchPhase::Bubble {
                                let _ = letting.update(cx, VideoPanel::release);
                            }
                        });
                    },
                )
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
            // A press on a control is the control's, not a hold of the panel.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
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
