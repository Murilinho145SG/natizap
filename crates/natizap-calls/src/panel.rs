//! A video call inside the main window: a small floating picture over the
//! corner, the other end's video with the user's own in its corner, so
//! chatting goes on beside it. Clicking it moves the call to a window of
//! its own, where the video is large.

use gpui::prelude::*;
use gpui::{Context, Entity, EventEmitter, FontWeight, Window, div, px, rgb, rgba};
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

/// The video of the call under way, floating over the window's corner.
/// Draws nothing without video.
pub struct VideoPanel {
    desk: Entity<CallDesk>,
    pictures: Entity<ProfilePictures>,
    remote: Painted,
    preview: Painted,
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
        }
    }
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
        div()
            .id("call-video-panel")
            .group("call-video-panel")
            .absolute()
            .bottom_4()
            .right_4()
            .w(px(WIDTH))
            .h(px(HEIGHT))
            .occlude()
            .cursor_pointer()
            .on_click(cx.listener(|_, _, _, cx| cx.emit(CallPoppedOut)))
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
            )
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
    width: gpui::Pixels,
    height: gpui::Pixels,
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
