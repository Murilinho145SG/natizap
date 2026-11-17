//! The composer under a conversation: attach, emoji, the text field and
//! send, with the reply being composed above them.

use gpui::prelude::*;
use gpui::{
    Context, Div, MouseButton, PathPromptOptions, Stateful, Window, deferred, div, px, relative,
    rgb, svg,
};
use natizap_protocol::ContentKind;
use natizap_ui::theme::{LINE, MUTED, PANEL, RAISED, TEXT, glass};
use natizap_ui::{Button, ButtonSize, ButtonVariant, Icon};

use super::ConversationView;
use crate::bubble::quote_block;

impl ConversationView {
    /// Sends what the composer holds, if anything, and empties it.
    pub(super) fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).text().trim().to_owned();
        if text.is_empty() {
            return;
        }
        let reply = self.replying.take().map(|replying| replying.reply);
        self.log.update(cx, |log, cx| log.send(text, reply, cx));
        self.composer
            .update(cx, |composer, cx| composer.clear(window, cx));
    }

    /// Asks for files and sends them. Text in the composer becomes the first
    /// file's caption, and a reply being composed applies to it.
    fn attach(window: &mut Window, cx: &mut Context<Self>) {
        let picking = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Send".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let paths = match picking.await {
                Ok(Ok(Some(paths))) if !paths.is_empty() => paths,
                Ok(Ok(_)) | Err(_) => return,
                Ok(Err(error)) => {
                    tracing::warn!(%error, "could not ask for files");
                    return;
                }
            };
            let _ = this.update_in(cx, |this, window, cx| {
                let text = this.composer.read(cx).text().trim().to_owned();
                let caption = (!text.is_empty()).then_some(text);
                if caption.is_some() {
                    this.composer
                        .update(cx, |composer, cx| composer.clear(window, cx));
                }
                let reply = this.replying.take().map(|replying| replying.reply);
                this.log
                    .update(cx, |log, cx| log.send_files(paths, caption, reply, cx));
            });
        })
        .detach();
    }

    fn reply_bar(&self, cx: &mut Context<Self>) -> Option<Div> {
        let replying = self.replying.as_ref()?;
        Some(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    quote_block(
                        replying.author.clone(),
                        replying.color,
                        replying.reply.text.clone().into(),
                        ContentKind::Text,
                        None,
                    )
                    .flex_1()
                    .min_w_0(),
                )
                .child(
                    div()
                        .id("cancel-reply")
                        .size(px(28.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(RAISED)))
                        // Acts on press, so nothing else can claim the click
                        // first.
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                cx.stop_propagation();
                                this.replying = None;
                                cx.notify();
                            }),
                        )
                        .child(
                            svg()
                                .path(Icon::Close.path())
                                .size(px(16.0))
                                .text_color(rgb(MUTED)),
                        ),
                ),
        )
    }

    pub(super) fn composer(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("composer")
            .relative()
            .flex_none()
            .flex()
            .flex_col()
            .gap_2()
            .px_4()
            .py_3()
            .bg(rgb(PANEL))
            .border_t_1()
            .border_color(rgb(LINE))
            .when(self.picking_emoji, |composer| {
                // Floats above the messages instead of growing the composer,
                // which pushed the composer out of a tall message list.
                composer.child(deferred(
                    div()
                        .absolute()
                        .bottom(relative(1.0))
                        .left_4()
                        .mb_2()
                        .occlude()
                        .child(self.emoji_picker.clone()),
                ))
            })
            .children(self.reply_bar(cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        Button::new("attach")
                            .icon(Icon::Plus)
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Large)
                            .round()
                            .tooltip("Attach")
                            .on_click(cx.listener(|_, _, window, cx| Self::attach(window, cx))),
                    )
                    .child(
                        Button::new("emoji")
                            .icon(Icon::Emoji)
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Large)
                            .round()
                            .selected(self.picking_emoji)
                            .tooltip("Emoji")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.picking_emoji = !this.picking_emoji;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px_4()
                            .py_2p5()
                            .rounded_full()
                            .bg(rgb(RAISED))
                            .map(glass)
                            .text_size(px(14.5))
                            .text_color(rgb(TEXT))
                            .child(self.composer.clone()),
                    )
                    .child(
                        Button::new("send")
                            .icon(Icon::Send)
                            .size(ButtonSize::Large)
                            .round()
                            .tooltip("Send")
                            .on_click(cx.listener(|this, _, window, cx| this.send(window, cx))),
                    ),
            )
    }
}
