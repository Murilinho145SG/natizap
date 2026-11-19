//! A call ringing here, as a card floating over the main window's top
//! right corner: who, and the answers.

use gpui::prelude::*;
use gpui::{Context, Div, Entity, FontWeight, SharedString, Window, div, px, rgb, svg};
use natizap_profiles::ProfilePictures;
use natizap_ui::motion::rise_in;
use natizap_ui::theme::{ActiveTheme as _, DISPLAY_FONT, Theme};
use natizap_ui::{Button, ButtonSize, ButtonVariant, Icon, avatar};

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
        // Laid out as a phone rings: what it is, who, and the two answers
        // as round buttons named under them.
        let decline = answer(
            (
                "ring-decline",
                Icon::CallEnd,
                ButtonVariant::Danger,
                "Decline",
            ),
            theme,
            cx.listener(|card, _, _, cx| card.desk.update(cx, CallDesk::decline)),
        );
        let take = answer(
            (
                "ring-answer",
                if ring.video { Icon::Video } else { Icon::Call },
                ButtonVariant::Primary,
                "Answer",
            ),
            theme,
            cx.listener(|card, _, _, cx| card.desk.update(cx, CallDesk::answer)),
        );
        div()
            .absolute()
            .top_3()
            .right_3()
            .w(px(280.0))
            .occlude()
            .child(rise_in(
                SharedString::from(format!("ring-card-{}", ring.id)),
                theme
                    .glass(div())
                    .px_5()
                    .pt_4()
                    .pb_5()
                    .flex()
                    .flex_col()
                    .items_center()
                    .rounded_2xl()
                    .bg(rgb(theme.raised))
                    .child(kind_line(ring.video, theme))
                    .child(div().pt_5().pb_3().child(ringed(
                        SharedString::from(format!("ring-pulse-{}", ring.id)),
                        avatar(ring.caller.as_str(), &title, picture, px(72.0)),
                        72.0,
                        true,
                        theme,
                    )))
                    .child(
                        div()
                            .max_w_full()
                            .truncate()
                            .font_family(DISPLAY_FONT)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(19.0))
                            .child(title),
                    )
                    .when(in_call, |card| {
                        card.child(
                            div()
                                .pt_1()
                                .text_size(px(12.0))
                                .text_color(rgb(theme.muted))
                                .child("Answering ends your call"),
                        )
                    })
                    .child(
                        div()
                            .pt_5()
                            .w_full()
                            .flex()
                            .justify_around()
                            .child(decline)
                            .child(take),
                    ),
                -12.0,
            ))
            .into_any_element()
    }
}

/// A round answer to the ringing call, named under it.
fn answer(
    (id, icon, variant, label): (&'static str, Icon, ButtonVariant, &'static str),
    theme: Theme,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap_1p5()
        .child(
            Button::new(id)
                .icon(icon)
                .variant(variant)
                .size(ButtonSize::Large)
                .round()
                .tooltip(label)
                .on_click(on_click),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(rgb(theme.muted))
                .child(label),
        )
}

/// What rings, above the caller.
fn kind_line(video: bool, theme: Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1p5()
        .text_size(px(12.0))
        .text_color(rgb(theme.muted))
        .child(
            svg()
                .path(if video { Icon::Video } else { Icon::Call }.path())
                .size(px(13.0))
                .text_color(rgb(theme.accent)),
        )
        .child(if video {
            "Incoming video call"
        } else {
            "Incoming voice call"
        })
}
