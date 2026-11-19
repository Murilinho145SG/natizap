//! Files dragged over the conversation: dropping them shows them before
//! sending.

use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt as _, ExternalPaths, FontWeight, bounce, div, ease_in_out, px, rgb,
    rgba, svg,
};
use natizap_ui::Icon;
use natizap_ui::theme::Theme;

use crate::send_preview::MOST_FILES;

/// How long the zone takes to appear.
const FADE_IN: Duration = Duration::from_millis(160);
/// One rise and fall of the arrow.
const BOB: Duration = Duration::from_millis(1100);
/// Side of the round badge the tray and arrow sit in.
const BADGE: f32 = 72.0;
/// Side of the tray and arrow.
const ICON: f32 = 34.0;
/// Where the tray and arrow sit in the badge, centered.
const ICON_INSET: f32 = (BADGE - ICON) / 2.0;
/// How far the arrow rises above the tray, in pixels.
const BOB_HEIGHT: f32 = 6.0;

/// Shown over the conversation while files are dragged over the window,
/// so the user knows they can be dropped here: it fades in, its arrow
/// keeps dropping into the tray, and it lights up once the files are over it.
pub(super) fn drop_zone(theme: Theme) -> impl IntoElement {
    div()
        .absolute()
        .inset_0()
        .p_3()
        // The messages show faintly through, so it reads as a layer over
        // the chat rather than a new screen.
        .bg(rgba((theme.pane << 8) | 0xf5))
        .child(
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .rounded_xl()
                .border_2()
                .border_dashed()
                .border_color(rgb(theme.line))
                .group_drag_over::<ExternalPaths>("conversation", |zone| {
                    zone.border_color(rgb(theme.accent))
                        .bg(rgba((theme.accent << 8) | 0x0f))
                })
                .child(
                    div()
                        .relative()
                        .mb_2()
                        .size(px(BADGE))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(rgba((theme.accent << 8) | 0x1f))
                        .child(
                            svg()
                                .absolute()
                                .top(px(ICON_INSET))
                                .left(px(ICON_INSET))
                                .size(px(ICON))
                                .path(Icon::Tray.path())
                                .text_color(rgb(theme.accent)),
                        )
                        .child(
                            svg()
                                .absolute()
                                .left(px(ICON_INSET))
                                .size(px(ICON))
                                .path(Icon::ArrowDown.path())
                                .text_color(rgb(theme.accent))
                                .with_animation(
                                    "drop-bob",
                                    Animation::new(BOB)
                                        .repeat()
                                        .with_easing(bounce(ease_in_out)),
                                    |arrow, delta| arrow.top(px(ICON_INSET - BOB_HEIGHT * delta)),
                                ),
                        ),
                )
                .child(
                    div()
                        .text_size(px(16.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(theme.text))
                        .child("Drop files to send them"),
                )
                .child(
                    div()
                        .text_size(px(13.0))
                        .text_color(rgb(theme.muted))
                        .child(format!(
                            "Photos, videos and documents, up to {MOST_FILES} at once"
                        )),
                ),
        )
        .with_animation("drop-fade-in", Animation::new(FADE_IN), |zone, delta| {
            zone.opacity(delta)
        })
}
