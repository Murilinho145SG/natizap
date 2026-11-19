//! A person's chat's encryption: the profile's row that opens the chat's
//! security code, for the two people to compare, each on their own
//! device, that their messages and calls reach only each other.

use gpui::prelude::*;
use gpui::{ClipboardItem, Context, Div, FontWeight, SharedString, Window, div, px, rgb, svg};
use natizap_protocol::SecurityCode;
use natizap_ui::theme::{ActiveTheme as _, MONO_FONT, Theme};
use natizap_ui::{Button, ButtonVariant, Icon, dialog};

use crate::panel::ProfilePanel;

/// The security code, from asking for it until the dialog closes.
#[derive(Debug, Clone)]
pub(crate) enum CodeShown {
    /// Being worked out: every device's keys are fetched first.
    Working,
    /// The code.
    Found(SecurityCode),
    /// It could not be had, and why.
    Failed(SharedString),
}

impl ProfilePanel {
    /// The encryption row: for a person's chat, it opens the code.
    pub(crate) fn encryption_row(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = *cx.theme();
        let verifiable = self.has_security_code();
        // Two lines, as on the phone: what it is, and what it means.
        div()
            .id("encryption")
            .w_full()
            .px_2()
            .py_3()
            .flex()
            .items_center()
            .gap_4()
            .rounded_lg()
            .child(
                svg()
                    .path(Icon::Lock.path())
                    .size(px(18.0))
                    .flex_none()
                    .text_color(rgb(theme.muted)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .text_size(px(14.5))
                            .text_color(rgb(theme.text))
                            .child("Encryption"),
                    )
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(rgb(theme.muted))
                            .child(if verifiable {
                                "Messages and calls are end-to-end encrypted. Click to verify."
                            } else {
                                "Messages and calls are end-to-end encrypted."
                            }),
                    ),
            )
            .when(verifiable, |row| {
                row.cursor_pointer()
                    .hover(|row| row.bg(rgb(theme.raised)))
                    .on_click(cx.listener(|panel, _, _, cx| panel.verify(cx)))
            })
    }

    /// Opens the dialog and works the code out.
    pub(crate) fn verify(&mut self, cx: &mut Context<Self>) {
        self.code = Some(CodeShown::Working);
        cx.notify();
        let working = self.protocol.read(cx).security_code(self.chat(), cx);
        cx.spawn(async move |panel, cx| {
            let shown = match working.await {
                Ok(code) => CodeShown::Found(code),
                Err(error) => {
                    tracing::warn!(%error, "no security code");
                    CodeShown::Failed(
                        "The security code could not be worked out. Check the connection \
                         and try again."
                            .into(),
                    )
                }
            };
            let _ = panel.update(cx, |panel, cx| {
                // Closed while it was worked out.
                if panel.code.is_some() {
                    panel.code = Some(shown);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The dialog showing the code, while it is open.
    pub(crate) fn code_dialog(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let theme = *cx.theme();
        let shown = self.code.clone()?;
        let close = {
            let panel = cx.weak_entity();
            move |_: &mut Window, cx: &mut gpui::App| {
                let _ = panel.update(cx, |panel, cx| {
                    panel.code = None;
                    cx.notify();
                });
            }
        };
        let closing = close.clone();
        let copied = match &shown {
            CodeShown::Found(code) => Some(code.groups().collect::<Vec<_>>().join(" ")),
            _ => None,
        };
        let body = match shown {
            CodeShown::Working => note("Working out the security code…", theme),
            CodeShown::Failed(why) => note(why, theme),
            CodeShown::Found(code) => digits(&code, theme),
        };
        let panel = div()
            .id("security-code")
            .w(px(400.0))
            .p_5()
            .flex()
            .flex_col()
            .gap_3()
            .rounded_xl()
            .bg(rgb(theme.panel))
            .map(|surface| theme.glass(surface))
            .occlude()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        svg()
                            .path(Icon::Lock.path())
                            .size(px(16.0))
                            .text_color(rgb(theme.mint)),
                    )
                    .child(
                        div()
                            .text_size(px(16.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Verify security code"),
                    ),
            )
            .child(
                div()
                    .text_size(px(13.5))
                    .text_color(rgb(theme.muted))
                    .child(format!(
                        "Messages and calls with {} are end-to-end encrypted. To be sure \
                         they reach only them, compare these numbers with the ones on \
                         their phone, in this chat's contact info, under Encryption.",
                        self.title()
                    )),
            )
            .child(body)
            .child(
                div()
                    .pt_2()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .children(copied.map(|text| {
                        Button::new("copy-security-code")
                            .label("Copy")
                            .variant(ButtonVariant::Ghost)
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                            })
                    }))
                    .child(
                        Button::new("close-security-code")
                            .label("Done")
                            .variant(ButtonVariant::Secondary)
                            .on_click(move |_, window, cx| closing(window, cx)),
                    ),
            );
        Some(dialog(window, panel, close, theme).into_any_element())
    }
}

/// The code's twelve groups, four to a line, as the phone shows them.
fn digits(code: &SecurityCode, theme: Theme) -> Div {
    let groups: Vec<&str> = code.groups().collect();
    div()
        .py_3()
        .flex()
        .flex_col()
        .items_center()
        .gap_2()
        .rounded_lg()
        .bg(rgb(theme.deep))
        .font_family(MONO_FONT)
        .text_size(px(18.0))
        .children(groups.chunks(4).map(|line| {
            div()
                .flex()
                .gap_4()
                .children(line.iter().map(|group| div().child(group.to_string())))
        }))
}

/// A line in place of the code: while it is worked out, or when it could
/// not be.
fn note(text: impl Into<SharedString>, theme: Theme) -> Div {
    div()
        .h(px(112.0))
        .flex()
        .items_center()
        .justify_center()
        .text_align(gpui::TextAlign::Center)
        .text_size(px(13.5))
        .text_color(rgb(theme.muted))
        .child(text.into())
}
