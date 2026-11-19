//! Small pieces every view of a call draws alike: the bars that tell the
//! connection's quality, and the ring that pulses round a picture while a
//! call rings or the other end speaks.

use gpui::prelude::*;
use gpui::{Animation, AnimationExt as _, AnyElement, Div, ElementId, div, px, rgb};
use natizap_ui::motion::BREATH;
use natizap_ui::theme::Theme;

use crate::quality::Quality;

/// Three bars, rising, as many lit as the connection deserves, in its
/// state's colour.
pub(crate) fn quality_bars(quality: Quality, theme: Theme) -> Div {
    let (lit, color) = match quality {
        Quality::Good => (3, theme.mint),
        Quality::Fair => (2, theme.accent),
        Quality::Poor => (1, theme.alert),
    };
    div().flex().items_end().gap(px(1.5)).h(px(10.0)).children(
        [4.0, 7.0, 10.0]
            .into_iter()
            .enumerate()
            .map(|(index, height)| {
                div()
                    .w(px(3.0))
                    .h(px(height))
                    .rounded(px(1.0))
                    .bg(rgb(if index < lit { color } else { theme.line }))
            }),
    )
}

/// What a connection's quality says in words.
pub(crate) fn quality_word(quality: Quality) -> &'static str {
    match quality {
        Quality::Good => "Good connection",
        Quality::Fair => "Unsteady connection",
        Quality::Poor => "Poor connection",
    }
}

/// `picture`, `size` across, with a ring pulsing out of it while `pulsing`.
pub(crate) fn ringed(
    id: impl Into<ElementId>,
    picture: Div,
    size: f32,
    pulsing: bool,
    theme: Theme,
) -> AnyElement {
    let reach = (size * 0.12).max(6.0);
    div()
        .relative()
        .size(px(size))
        .flex_none()
        .when(pulsing, |ringed| {
            ringed.child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded_full()
                    .border_2()
                    .border_color(rgb(theme.accent))
                    .with_animation(id, Animation::new(BREATH).repeat(), move |ring, delta| {
                        let out = px(-reach * delta);
                        ring.top(out)
                            .left(out)
                            .right(out)
                            .bottom(out)
                            .opacity(0.7 * (1.0 - delta))
                    }),
            )
        })
        .child(picture)
        .into_any_element()
}
