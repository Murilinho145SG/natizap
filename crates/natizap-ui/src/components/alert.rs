//! A banner telling the user about a state of the app, e.g. a lost
//! connection.

use gpui::prelude::*;
use gpui::{App, FontWeight, SharedString, Window, div, px, rgb, svg};

use crate::Icon;
use crate::theme::{ACCENT, ALERT, MINT, MUTED, RAISED, TEXT, glass};

/// What kind of news an alert brings, which sets its color.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AlertTone {
    /// Something worth knowing.
    #[default]
    Info,
    /// Something degraded that natizap is handling, e.g. reconnecting.
    Warning,
    /// Something failed and needs the user.
    Error,
}

/// A banner with a title and an optional description.
#[derive(Debug, IntoElement)]
pub struct Alert {
    tone: AlertTone,
    title: SharedString,
    description: Option<SharedString>,
}

impl Alert {
    /// An alert saying `title`.
    pub fn new(tone: AlertTone, title: impl Into<SharedString>) -> Self {
        Self {
            tone,
            title: title.into(),
            description: None,
        }
    }

    /// A line of detail under the title.
    #[must_use]
    pub fn description(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self
    }
}

impl RenderOnce for Alert {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let color = match self.tone {
            AlertTone::Info => MINT,
            AlertTone::Warning => ACCENT,
            AlertTone::Error => ALERT,
        };
        div()
            .flex()
            .items_start()
            .gap_3()
            .px_4()
            .py_3()
            .rounded_xl()
            .bg(rgb(RAISED))
            .map(glass)
            .child(
                svg()
                    .path(Icon::Alert.path())
                    .size(px(18.0))
                    .mt(px(1.0))
                    .text_color(rgb(color)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(
                        div()
                            .text_size(px(14.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(TEXT))
                            .child(self.title),
                    )
                    .children(
                        self.description.map(|text| {
                            div().text_size(px(13.0)).text_color(rgb(MUTED)).child(text)
                        }),
                    ),
            )
    }
}
