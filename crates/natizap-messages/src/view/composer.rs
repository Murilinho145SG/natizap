//! The composer under a conversation: attach, emoji, the text field and
//! send, with the reply being composed above them.

use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{
    ClickEvent, Context, Div, Image, ImageFormat, MouseButton, PathPromptOptions, Stateful, Window,
    deferred, div, px, relative, rgb, svg,
};
use natizap_protocol::ContentKind;
use natizap_ui::theme::{LINE, MUTED, PANEL, RAISED, TEXT, glass};
use natizap_ui::{Button, ButtonSize, ButtonVariant, FilesPasted, Icon};

use super::ConversationView;
use crate::quote::quote_block;

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

    /// Sends files and pictures pasted into the composer, as the phone
    /// sends shared files: text in the composer becomes the first one's
    /// caption. Pictures, which have no file, are written to one first.
    pub(super) fn send_pasted(pasted: &FilesPasted, window: &mut Window, cx: &mut Context<Self>) {
        let mut paths = pasted.paths.clone();
        let images = pasted.images.clone();
        cx.spawn_in(window, async move |this, cx| {
            let written = cx
                .background_spawn(async move {
                    images
                        .iter()
                        .enumerate()
                        .filter_map(|(index, image)| match write_pasted(image, index) {
                            Ok(path) => Some(path),
                            Err(error) => {
                                tracing::warn!(%error, "could not keep a pasted picture");
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            paths.extend(written);
            if paths.is_empty() {
                return;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                this.send_paths(paths, false, window, cx);
            });
        })
        .detach();
    }

    /// Sends the files at `paths`, as documents when `as_document`, the
    /// first captioned with the composer's text and replying to the reply
    /// being composed.
    fn send_paths(
        &mut self,
        paths: Vec<PathBuf>,
        as_document: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self.composer.read(cx).text().trim().to_owned();
        let caption = (!text.is_empty()).then_some(text);
        if caption.is_some() {
            self.composer
                .update(cx, |composer, cx| composer.clear(window, cx));
        }
        let reply = self.replying.take().map(|replying| replying.reply);
        self.log.update(cx, |log, cx| {
            log.send_files(paths, caption, reply, as_document, cx);
        });
    }

    /// Asks for files and sends them, as documents when `as_document`. Text
    /// in the composer becomes the first file's caption, and a reply being
    /// composed applies to it.
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
                this.send_paths(paths, as_document, window, cx);
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

/// Writes a pasted picture to a file of its own, to send like any other.
fn write_pasted(image: &Image, index: usize) -> std::io::Result<PathBuf> {
    let extension = match image.format() {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpg",
        ImageFormat::Webp => "webp",
        ImageFormat::Gif => "gif",
        ImageFormat::Bmp => "bmp",
        ImageFormat::Tiff => "tiff",
        ImageFormat::Svg => "svg",
        ImageFormat::Ico => "ico",
        ImageFormat::Pnm => "pnm",
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    let path = std::env::temp_dir().join(format!("natizap-pasted-{stamp}-{index}.{extension}"));
    std::fs::write(&path, image.bytes())?;
    Ok(path)
}
