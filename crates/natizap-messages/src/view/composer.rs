//! The composer under a conversation: attach, emoji, the text field and
//! send, with the reply being composed above them.

use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{
    ClickEvent, Context, Div, Focusable as _, MouseButton, PathPromptOptions, SharedString,
    Stateful, Window, deferred, div, px, relative, rgb, svg,
};
use natizap_protocol::ContentKind;
use natizap_ui::motion::{fade_in, rise_in};
use natizap_ui::theme::{LINE, MUTED, PANEL, RAISED, TEXT, glass};
use natizap_ui::{Button, ButtonSize, ButtonVariant, FilesPasted, Icon};

use super::ConversationView;
use crate::quote::quote_block;
use crate::send_preview::{PreviewClosed, PreviewSent, SendPreview, write_pasted};

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

    /// Shows files and pictures pasted into the composer before sending
    /// them. Pictures, which have no file, are written to one first.
    pub(super) fn stage_pasted(pasted: &FilesPasted, window: &mut Window, cx: &mut Context<Self>) {
        let mut paths = pasted.paths.clone();
        let images = pasted.images.clone();
        cx.spawn_in(window, async move |this, cx| {
            let written = cx
                .background_spawn(async move { write_pasted(&images) })
                .await;
            paths.extend(written);
            let _ = this.update_in(cx, |this, window, cx| {
                this.stage(paths, false, window, cx);
            });
        })
        .detach();
    }

    /// Shows `paths` before sending them, as documents when `as_document`,
    /// adding them to the files already shown. Text in the composer becomes
    /// the first file's caption.
    pub(super) fn stage(
        &mut self,
        paths: Vec<PathBuf>,
        as_document: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() || self.log.read(cx).open_chat().is_none() {
            return;
        }
        if let Some(preview) = &self.sending {
            preview.update(cx, |preview, cx| preview.add(paths, cx));
            return;
        }
        let caption = self.composer.read(cx).text().trim().to_owned();
        self.composer
            .update(cx, |composer, cx| composer.clear(window, cx));
        let preview = cx.new(|cx| SendPreview::new(paths, as_document, caption, window, cx));
        cx.subscribe_in(
            &preview,
            window,
            |this, _, sent: &PreviewSent, window, cx| {
                let reply = this.replying.take().map(|replying| replying.reply);
                let PreviewSent { files, as_document } = sent.clone();
                this.log.update(cx, |log, cx| {
                    log.send_files(files, reply, as_document, cx);
                });
                this.close_preview(window, cx);
            },
        )
        .detach();
        cx.subscribe_in(
            &preview,
            window,
            |this, _, _: &PreviewClosed, window, cx| {
                this.close_preview(window, cx);
            },
        )
        .detach();
        window.focus(&preview.read(cx).focus_handle(cx), cx);
        self.sending = Some(preview);
        cx.notify();
    }

    /// Goes back from the files about to be sent to the conversation.
    pub(super) fn close_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sending.take().is_some() {
            window.focus(&self.composer.focus_handle(cx), cx);
            cx.notify();
        }
    }

    /// Asks for files and shows them before sending, as documents when
    /// `as_document`.
    pub(super) fn attach(as_document: bool, window: &mut Window, cx: &mut Context<Self>) {
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
                this.stage(paths, as_document, window, cx);
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
                        None,
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

    /// Why this account cannot write here, when only admins may and it is
    /// not one: shown where the composer would be, as on the phone.
    fn who_may_write(&self, cx: &Context<Self>) -> Option<&'static str> {
        let group = self.log.read(cx).group()?;
        (group.admins_only && !group.own_admin).then_some(if group.community_announcements {
            "Only community admins can send messages"
        } else {
            "Only admins can send messages"
        })
    }

    pub(super) fn composer(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        if let Some(notice) = self.who_may_write(cx) {
            return div()
                .id("composer")
                .flex_none()
                .flex()
                .justify_center()
                .px_4()
                .py_4()
                .bg(rgb(PANEL))
                .border_t_1()
                .border_color(rgb(LINE))
                .text_size(px(13.5))
                .text_color(rgb(MUTED))
                .child(notice);
        }
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
                        .child(rise_in(
                            "emoji-picker-in",
                            div().child(self.emoji_picker.clone()),
                            8.0,
                        )),
                ))
            })
            .children(self.reply_bar(cx).map(|bar| {
                let id = self
                    .replying
                    .as_ref()
                    .map_or_default(|replying| replying.reply.id.clone());
                rise_in(SharedString::from(format!("reply-{id}")), bar, 8.0)
            }))
            .when(self.recording(), |composer| {
                composer.child(rise_in("recording-in", self.recording_bar(cx), 8.0))
            })
            .when(!self.recording(), |composer| {
                composer.child(self.writing_bar(cx))
            })
    }

    /// Attach, emoji, the text field, and send or record.
    fn writing_bar(&self, cx: &mut Context<Self>) -> Div {
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
                    .on_click(cx.listener(|this, event: &ClickEvent, _, cx| {
                        this.attach_at = Some(event.position());
                        cx.notify();
                    })),
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
            // Send and record swap places as the field fills and empties;
            // each fades in, so the swap reads as one control changing.
            .child(fade_in(
                if self.composer.read(cx).text().trim().is_empty() {
                    "record-in"
                } else {
                    "send-in"
                },
                div().child(self.send_or_record(cx)),
            ))
    }
}
