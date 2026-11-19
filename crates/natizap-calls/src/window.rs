//! A call in a window of its own: the person large in the middle, the
//! call's clock under them and its controls at the bottom. The window
//! closes when the call leaves the screen.

use gpui::prelude::*;
use gpui::{Context, Entity, FontWeight, Window, div, px, rgb};
use natizap_profiles::ProfilePictures;
use natizap_ui::theme::{ActiveTheme as _, DISPLAY_FONT, base_font};
use natizap_ui::{Button, ButtonSize, ButtonVariant, Icon, avatar, window_frame};

use crate::bar::{mute_button, phase_line};
use crate::desk::{CallDesk, Phase};

/// The call under way, filling a window.
pub struct CallWindow {
    desk: Entity<CallDesk>,
    pictures: Entity<ProfilePictures>,
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
        Self { desk, pictures }
    }
}

impl Render for CallWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let call = self.desk.read(cx).call(cx);
        let content =
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .bg(rgb(theme.pane))
                .children(call.map(|call| {
                    let picture = self
                        .pictures
                        .update(cx, |pictures, cx| pictures.picture(&call.peer, cx));
                    let ended = matches!(call.phase, Phase::Ended(_));
                    div()
                        .size_full()
                        .flex()
                        .flex_col()
                        .items_center()
                        .px_6()
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .gap_3()
                                .child(avatar(call.peer.as_str(), &call.title, picture, px(112.0)))
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
                                .child(phase_line(&call, theme)),
                        )
                        .child(div().pb_8().flex().items_center().gap_4().when(
                            !ended,
                            |controls| {
                                controls
                                    .child(mute_button(
                                        &call,
                                        ButtonSize::Large,
                                        cx.listener(|this, _, _, cx| {
                                            this.desk.update(cx, CallDesk::toggle_muted);
                                        }),
                                    ))
                                    .child(
                                        Button::new("call-window-hang-up")
                                            .icon(Icon::CallEnd)
                                            .variant(ButtonVariant::Danger)
                                            .size(ButtonSize::Large)
                                            .round()
                                            .tooltip("Hang up")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.desk.update(cx, CallDesk::hang_up);
                                            })),
                                    )
                            },
                        ))
                }));
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
