//! A call in a window of its own. A voice call shows the person large in
//! the middle with the call's clock under them; a video call fills the
//! window with the other end's picture, the user's own in its corner.
//! The controls sit at the bottom. The window closes when the call leaves
//! the screen.

use gpui::prelude::*;
use gpui::{Context, Entity, FontWeight, Window, div, px, rgb, rgba};
use natizap_profiles::ProfilePictures;
use natizap_ui::theme::{ActiveTheme as _, DISPLAY_FONT, Theme, base_font};
use natizap_ui::{Button, ButtonSize, ButtonVariant, Icon, avatar, window_frame};

use crate::bar::{camera_button, mute_button, phase_line};
use crate::desk::{CallDesk, CallShown, Phase};
use crate::keys::HANG_UP;
use crate::look::ringed;
use crate::panel::own_picture;
use crate::screen::Painted;

/// The call under way, filling a window.
pub struct CallWindow {
    desk: Entity<CallDesk>,
    pictures: Entity<ProfilePictures>,
    remote: Painted,
    preview: Painted,
}

impl std::fmt::Debug for CallWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallWindow").finish_non_exhaustive()
    }
}

impl CallWindow {
    /// Shows `desk`'s call in `window`, closing it once the call is gone.
    pub fn new(
        desk: Entity<CallDesk>,
        pictures: Entity<ProfilePictures>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe_in(&desk, window, |_, desk, window, cx| {
            if desk.read(cx).call(cx).is_none() {
                window.remove_window();
            }
            cx.notify();
        })
        .detach();
        cx.observe(&pictures, |_, _, cx| cx.notify()).detach();
        Self {
            desk,
            pictures,
            remote: Painted::default(),
            preview: Painted::default(),
        }
    }

    /// The person large in the middle, while no picture of theirs shows.
    fn person(&self, call: &CallShown, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = *cx.theme();
        let picture = self
            .pictures
            .update(cx, |pictures, cx| pictures.picture(&call.peer, cx));
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .px_6()
            .child(ringed(
                "call-window-speaking",
                avatar(call.peer.as_str(), &call.title, picture, px(112.0)),
                112.0,
                call.speaking,
                theme,
            ))
            .child(
                div()
                    .pt_2()
                    .max_w_full()
                    .truncate()
                    .font_family(DISPLAY_FONT)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(24.0))
                    .child(call.title.clone()),
            )
            .child(phase_line(call, theme))
    }

    /// The call's controls, in a row.
    fn controls(call: &CallShown, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .flex()
            .items_center()
            .gap_4()
            .child(mute_button(
                call,
                ButtonSize::Large,
                cx.listener(|this, _, _, cx| this.desk.update(cx, CallDesk::toggle_muted)),
            ))
            .child(camera_button(
                call,
                ButtonSize::Large,
                cx.listener(|this, _, _, cx| this.desk.update(cx, CallDesk::toggle_camera)),
            ))
            .child(
                Button::new("call-window-hang-up")
                    .icon(Icon::CallEnd)
                    .variant(ButtonVariant::Danger)
                    .size(ButtonSize::Large)
                    .round()
                    .tooltip(HANG_UP.tip("Hang up"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.desk.update(cx, CallDesk::hang_up);
                    })),
            )
    }

    /// The call, as the window shows it.
    fn stage(
        &mut self,
        call: &CallShown,
        theme: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let ended = matches!(call.phase, Phase::Ended(_));
        let Some(video) = call.video.clone().filter(|_| !ended) else {
            self.remote.paint(None, px(0.0), window);
            self.preview.paint(None, px(0.0), window);
            return div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .bg(rgb(theme.pane))
                .child(self.person(call, cx))
                .child(
                    div()
                        .pb_8()
                        .when(!ended, |row| row.child(Self::controls(call, cx))),
                )
                .into_any_element();
        };
        let remote = self.remote.paint(video.remote.clone(), px(0.0), window);
        let preview = self.preview.paint(video.preview.clone(), px(10.0), window);
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(rgb(theme.deep))
            .child(remote)
            .when(video.remote.is_none(), |stage| {
                stage.child(self.person(call, cx))
            })
            .when(video.remote.is_some(), |stage| {
                stage.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .p_4()
                        .flex()
                        .flex_col()
                        .bg(rgba(theme.scrim))
                        .child(
                            div()
                                .truncate()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(call.title.clone()),
                        )
                        .child(phase_line(call, theme)),
                )
            })
            .child(
                div()
                    .absolute()
                    .right_4()
                    .bottom(px(88.0))
                    .child(own_picture(&video, preview, theme, px(160.0), px(120.0))),
            )
            .child(
                div()
                    .absolute()
                    .bottom_6()
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(Self::controls(call, cx)),
            )
            .into_any_element()
    }
}

impl Render for CallWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let content = match self.desk.read(cx).call(cx) {
            Some(call) => self.stage(&call, theme, window, cx),
            None => div().size_full().bg(rgb(theme.pane)).into_any_element(),
        };
        let title = div()
            .font_family(DISPLAY_FONT)
            .font_weight(FontWeight::BOLD)
            .text_size(px(15.0))
            .child("Call");
        div()
            .size_full()
            .font(base_font())
            .text_color(rgb(theme.text))
            .child(window_frame(window, title, content, theme))
    }
}
