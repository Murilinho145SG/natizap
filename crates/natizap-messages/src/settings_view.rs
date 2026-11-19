//! The chat settings' section of the settings window: how the conversation
//! shows who is typing.

use gpui::prelude::*;
use gpui::{Context, Entity, FontWeight, Window, div, px, rgb};
use natizap_ui::Switch;
use natizap_ui::theme::ActiveTheme as _;

use crate::log::{MessageLog, TypingLook};

/// Shows and changes how `log`'s conversation shows who is typing.
pub struct ChatSettings {
    log: Entity<MessageLog>,
}

impl std::fmt::Debug for ChatSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatSettings").finish_non_exhaustive()
    }
}

impl ChatSettings {
    /// Shows `log`'s chat settings.
    pub fn new(log: Entity<MessageLog>, cx: &mut Context<Self>) -> Self {
        cx.observe(&log, |_, _, cx| cx.notify()).detach();
        Self { log }
    }
}

impl Render for ChatSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let names = self.log.read(cx).typing_look() == TypingLook::NamesAndFaces;
        let log = self.log.clone();
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .px_5()
                    .pt_3()
                    .text_size(px(13.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(theme.muted))
                    .child("Chats"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_5()
                    .py_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .child("Names while typing")
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .text_color(rgb(theme.muted))
                                    .child("Say who is typing beside their pictures"),
                            ),
                    )
                    .child(
                        Switch::new("typing-names", names).on_toggle(move |on, _, cx| {
                            let look = if on {
                                TypingLook::NamesAndFaces
                            } else {
                                TypingLook::FacesOnly
                            };
                            log.update(cx, |log, cx| log.set_typing_look(look, cx));
                        }),
                    ),
            )
    }
}
