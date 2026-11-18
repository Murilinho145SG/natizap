//! What shows while natizap opens: its mark popping in, its name rising
//! under it and a thin bar sweeping while the app loads, then all of it
//! fading away into the app.

use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt as _, Div, FontWeight, Transformation, bounce, div, ease_in_out, px,
    rgb, size, svg,
};

use crate::Icon;
use crate::motion::{ease_out, rise_in_after};
use crate::theme::{ACCENT, DEEP, DISPLAY_FONT, LINE, TEXT};

/// How long the mark takes to pop in.
const MARK_IN: Duration = Duration::from_millis(520);
/// When the name starts rising, after the mark started.
const NAME_AFTER: Duration = Duration::from_millis(200);
/// When the loading bar shows: only if loading lasts beyond the intro.
const BAR_AFTER: Duration = Duration::from_millis(1200);
/// One sweep of the loading bar across its track.
const SWEEP: Duration = Duration::from_millis(1100);
/// How long the splash takes to fade away.
pub const SPLASH_OUT: Duration = Duration::from_millis(320);
/// The least time the splash shows, so its intro plays out.
pub const SPLASH_INTRO: Duration = Duration::from_millis(900);

const MARK: f32 = 84.0;
const TRACK: f32 = 120.0;
const SEGMENT: f32 = 36.0;

/// The splash filling the space it is given; `leaving` fades it away.
pub fn splash(leaving: bool) -> impl IntoElement {
    let layer = div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_4()
        .bg(rgb(DEEP))
        .child(mark(leaving))
        .child(rise_in_after(
            "splash-name",
            div()
                .font_family(DISPLAY_FONT)
                .font_weight(FontWeight::BOLD)
                .text_size(px(30.0))
                .text_color(rgb(TEXT))
                .child("natizap"),
            10.0,
            NAME_AFTER,
        ))
        .child(loading_bar());
    if leaving {
        layer
            .with_animation(
                "splash-out",
                Animation::new(SPLASH_OUT).with_easing(ease_out),
                |layer, delta| layer.opacity(1.0 - delta),
            )
            .into_any_element()
    } else {
        layer.into_any_element()
    }
}

/// The mark: popping in, overshooting a little and settling; growing a
/// touch as the splash leaves, as if it opened into the app.
fn mark(leaving: bool) -> impl IntoElement {
    let mark = svg()
        .path(Icon::Logo.path())
        .size(px(MARK))
        .text_color(rgb(ACCENT));
    if leaving {
        return mark
            .with_animation(
                "splash-mark-out",
                Animation::new(SPLASH_OUT).with_easing(ease_out),
                |mark, delta| {
                    let scale = 1.0 + 0.08 * delta;
                    mark.with_transformation(Transformation::scale(size(scale, scale)))
                },
            )
            .into_any_element();
    }
    mark.with_animation(
        "splash-mark-in",
        Animation::new(MARK_IN).with_easing(overshoot),
        |mark, delta| {
            let scale = 0.6 + 0.4 * delta;
            mark.with_transformation(Transformation::scale(size(scale, scale)))
                .opacity(delta.clamp(0.0, 1.0))
        },
    )
    .into_any_element()
}

/// A thin track with a segment sweeping back and forth: still loading. It
/// fades in late, so a quick start never shows it.
fn loading_bar() -> impl IntoElement {
    let travel = TRACK - SEGMENT;
    rise_in_after(
        "splash-bar",
        div()
            .relative()
            .w(px(TRACK))
            .h(px(3.0))
            .mt_4()
            .rounded_full()
            .overflow_hidden()
            .bg(rgb(LINE))
            .child(
                segment().with_animation(
                    "splash-sweep",
                    Animation::new(SWEEP)
                        .repeat()
                        .with_easing(bounce(ease_in_out)),
                    move |segment, delta| segment.left(px(travel * delta)),
                ),
            ),
        0.0,
        BAR_AFTER,
    )
}

fn segment() -> Div {
    div()
        .absolute()
        .top_0()
        .h_full()
        .w(px(SEGMENT))
        .rounded_full()
        .bg(rgb(ACCENT))
}

/// Eases out past the end and back, so the mark lands with a small pop.
fn overshoot(delta: f32) -> f32 {
    const PULL: f32 = 1.7;
    let t = delta - 1.0;
    1.0 + (PULL + 1.0) * t.powi(3) + PULL * t.powi(2)
}

#[cfg(test)]
mod tests {
    use super::overshoot;

    #[test]
    fn starts_and_lands_in_place() {
        assert!(overshoot(0.0).abs() < 1e-6);
        assert!((overshoot(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn goes_past_the_end_before_landing() {
        assert!(overshoot(0.8) > 1.0);
    }
}
