//! A question the user must answer before going on, e.g. confirming a
//! deletion, over the dimmed window.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{App, ClickEvent, FontWeight, SharedString, Window, div, px, rgb};

use crate::components::button::{Button, ButtonVariant};
use crate::components::dialog::dialog;
use crate::theme::ActiveTheme as _;

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

/// A modal question with its answers as buttons. Pressing outside it, on
/// the dimmed window, runs `on_dismiss`.
#[derive(IntoElement)]
pub struct AlertDialog {
    title: SharedString,
    description: Option<SharedString>,
    actions: Vec<(SharedString, ButtonVariant, Handler)>,
    on_dismiss: Handler,
}

impl std::fmt::Debug for AlertDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AlertDialog")
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

impl AlertDialog {
    /// A dialog asking `title`; `on_dismiss` runs when the user backs out.
    pub fn new(
        title: impl Into<SharedString>,
        on_dismiss: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            title: title.into(),
            description: None,
            actions: Vec::new(),
            on_dismiss: Rc::new(on_dismiss),
        }
    }

    /// A line explaining what the answers do.
    #[must_use]
    pub fn description(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// An answer; the last one added sits rightmost.
    #[must_use]
    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        variant: ButtonVariant,
        on_select: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.actions
            .push((label.into(), variant, Rc::new(on_select)));
        self
    }
}

impl RenderOnce for AlertDialog {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.theme();
        let dismiss = Rc::clone(&self.on_dismiss);
        let panel = div()
            .id("alert-dialog")
            .w(px(380.0))
            .p_5()
            .flex()
            .flex_col()
            .gap_2()
            .rounded_xl()
            .bg(rgb(theme.panel))
            .map(|surface| theme.glass(surface))
            .occlude()
            .child(
                div()
                    .text_size(px(16.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(theme.text))
                    .child(self.title),
            )
            .children(self.description.map(|text| {
                div()
                    .text_size(px(14.0))
                    .text_color(rgb(theme.muted))
                    .child(text)
            }))
            .child(
                div()
                    .pt_3()
                    .flex()
                    .flex_wrap()
                    .justify_end()
                    .gap_2()
                    .children(self.actions.into_iter().enumerate().map(
                        |(index, (label, variant, on_select))| {
                            Button::new(("alert-action", index))
                                .label(label)
                                .variant(variant)
                                .on_click(move |_: &ClickEvent, window, cx| on_select(window, cx))
                        },
                    )),
            );
        dialog(window, panel, move |window, cx| dismiss(window, cx), theme)
    }
}
