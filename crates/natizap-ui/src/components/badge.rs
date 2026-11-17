//! A small pill carrying a count or a short label.

use gpui::prelude::*;
use gpui::{Div, FontWeight, SharedString, Stateful, div, px, rgb, svg};

use crate::Icon;
use crate::components::tooltip::Tooltip;
use crate::theme::{ACCENT, ACCENT_INK, LINE, MINT, MUTED};

/// How loudly a badge speaks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BadgeTone {
    /// Wants attention, e.g. unread messages.
    #[default]
    Accent,
    /// Present but quiet, e.g. unread messages in a muted chat.
    Muted,
}

/// A pill with `text`; empty text makes a dot-sized pill, as for a chat
/// marked unread without a count.
#[must_use]
pub fn badge(text: impl Into<SharedString>, tone: BadgeTone) -> Div {
    let (background, ink) = match tone {
        BadgeTone::Accent => (ACCENT, ACCENT_INK),
        BadgeTone::Muted => (LINE, MUTED),
    };
    div()
        .min_w(px(20.0))
        .h(px(20.0))
        .px(px(6.0))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(rgb(background))
        .text_color(rgb(ink))
        .text_size(px(11.5))
        .font_weight(FontWeight::BOLD)
        .child(text.into())
}

/// The seal of a business WhatsApp vouches for, shown beside its name;
/// hovering it says what it means.
#[must_use]
pub fn verified_seal(size: f32) -> Stateful<Div> {
    div()
        .id("verified-seal")
        .flex_none()
        .tooltip(Tooltip::text("Verified business"))
        .child(
            svg()
                .path(Icon::Verified.path())
                .size(px(size))
                .text_color(rgb(MINT)),
        )
}

/// A count as a badge shows it: past 999 the exact number stops mattering.
#[must_use]
pub fn count_label(count: u32) -> String {
    if count > 999 {
        "999+".to_owned()
    } else {
        count.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_large_counts() {
        assert_eq!(count_label(7), "7");
        assert_eq!(count_label(999), "999");
        assert_eq!(count_label(1500), "999+");
    }
}
