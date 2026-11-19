//! A call ringing here, as a card floating over the main window's top
//! right corner: who, and the answers.

use gpui::prelude::*;
use gpui::{Context, Entity, FontWeight, SharedString, Window, div, px, rgb};
use natizap_profiles::ProfilePictures;
use natizap_ui::motion::rise_in;
use natizap_ui::theme::{ActiveTheme as _, DISPLAY_FONT};
use natizap_ui::{Button, ButtonVariant, Icon, avatar};

use crate::desk::CallDesk;
use crate::look::ringed;

/// The call ringing here, as a card floating over the window's top right
/// corner. Draws nothing while nothing rings.
pub struct RingCard {
    desk: Entity<CallDesk>,
    pictures: Entity<ProfilePictures>,
}

impl std::fmt::Debug for RingCard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RingCard").finish_non_exhaustive()
    }
}

impl RingCard {
    /// Shows the calls ringing on `desk`.
    pub fn new(
        desk: Entity<CallDesk>,
        pictures: Entity<ProfilePictures>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&desk, |_, _, cx| cx.notify()).detach();
        cx.observe(&pictures, |_, _, cx| cx.notify()).detach();
        Self { desk, pictures }
    }
}

impl Render for RingCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let Some(ring) = self.desk.read(cx).ringing().cloned() else {
            return div().into_any_element();
        };
        let title = self.desk.read(cx).title(&ring.caller, cx);
        let in_call = self.desk.read(cx).call(cx).is_some();
        let picture = self
            .pictures
            .update(cx, |pictures, cx| pictures.picture(&ring.caller, cx));
        let kind = if ring.video {
            "Video call"
        } else {
            "Voice call"
        };
        div()
            .absolute()
            .top_3()
            .right_3()
            .w(px(320.0))
            .occlude()
            .child(rise_in(
                SharedString::from(format!("ring-card-{}", ring.id)),
                theme
                    .glass(div())
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .rounded_xl()
                    .bg(rgb(theme.raised))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(ringed(
                                SharedString::from(format!("ring-pulse-{}", ring.id)),
                                avatar(ring.caller.as_str(), &title, picture, px(48.0)),
                                48.0,
                                true,
                                theme,
                            ))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .truncate()
                                            .font_family(DISPLAY_FONT)
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_size(px(17.0))
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .text_color(rgb(theme.muted))
                                            .child(kind),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                div().flex_1().child(
                                    Button::new("ring-decline")
                                        .icon(Icon::CallEnd)
                                        .label("Decline")
                                        .variant(ButtonVariant::Danger)
                                        .on_click(cx.listener(|card, _, _, cx| {
                                            card.desk.update(cx, CallDesk::decline);
                                        })),
                                ),
                            )
                            .child(
                                div().flex_1().child(
                                    Button::new("ring-answer")
                                        .icon(if ring.video { Icon::Video } else { Icon::Call })
                                        .label(if in_call { "End and answer" } else { "Answer" })
                                        .on_click(cx.listener(|card, _, _, cx| {
                                            card.desk.update(cx, CallDesk::answer);
                                        })),
                                ),
                            ),
                    ),
                -12.0,
            ))
            .into_any_element()
    }
}
