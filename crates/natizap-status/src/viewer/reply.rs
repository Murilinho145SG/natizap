//! The bar under a status: replies to someone's post with text, a quick
//! reaction or a voice note, or who saw your own.

use gpui::prelude::*;
use gpui::{Context, MouseButton, SharedString, Window, div, px, rgb, rgba};
use natizap_playback::Recorder;
use natizap_protocol::Reply;
use natizap_ui::theme::ActiveTheme as _;
use natizap_ui::{Button, ButtonVariant, Icon};

use super::{QUICK, ReplyBody, StatusReply, StatusViewer};
use crate::post::Post;

impl StatusViewer {
    pub(super) fn send_reply(
        &mut self,
        body: ReplyBody,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(post) = self.post(cx) else {
            return;
        };
        cx.emit(StatusReply {
            poster: post.poster.clone(),
            reply: Reply {
                id: post.id.clone(),
                sender: Some(post.poster.clone()),
                text: post.summary(),
                from_status: true,
            },
            body,
        });
        self.reply.update(cx, |input, cx| input.clear(window, cx));
        self.hold(false);
    }

    fn toggle_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.recorder.take() {
            Some(recorder) => {
                let heard = recorder.stop();
                self.send_reply(ReplyBody::Voice(heard), window, cx);
            }
            None => match Recorder::start() {
                Ok(recorder) => {
                    self.hold(true);
                    self.recorder = Some(recorder);
                }
                Err(error) => tracing::warn!(%error, "could not start recording a reply"),
            },
        }
        cx.notify();
    }

    /// Replying to someone's post, or who saw your own.
    pub(super) fn bottom(&self, post: &Post, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let bar = div().flex_none().px_6().pb_5().pt_2();
        if post.mine {
            self.seen_by(post, bar, cx)
        } else {
            self.reply_bar(bar, cx)
        }
    }

    /// Who saw your post, on a press.
    fn seen_by(&self, post: &Post, bar: gpui::Div, cx: &mut Context<Self>) -> gpui::AnyElement {
        let viewers = self.book.read(cx).viewers_of(&post.id);
        let names: Vec<SharedString> = viewers
            .iter()
            .map(|viewer| self.book.read(cx).title(&viewer.viewer))
            .collect();
        let count = names.len();
        bar.flex()
            .flex_col()
            .items_center()
            .gap_2()
            .when(self.viewers_open, |bar| {
                bar.child(
                    div()
                        .max_h(px(220.0))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .text_size(px(14.0))
                        .children(names),
                )
            })
            .child(
                Button::new("status-viewers")
                    .label(format!("Seen by {count}"))
                    .variant(ButtonVariant::Ghost)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.viewers_open = !this.viewers_open;
                        this.hold(this.viewers_open);
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// Replying with text, a quick reaction or a voice note.
    fn reply_bar(&self, bar: gpui::Div, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = *cx.theme();
        let recording = self.recorder.is_some();
        let quick = QUICK.iter().map(|emoji| {
            let (emoji, sent) = ((*emoji).to_owned(), (*emoji).to_owned());
            div()
                .id(SharedString::from(format!("quick-{emoji}")))
                .text_size(px(24.0))
                .cursor_pointer()
                .hover(|quick| quick.opacity(0.8))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.send_reply(ReplyBody::Text(sent.clone()), window, cx);
                }))
                .child(emoji.clone())
        });
        bar.flex()
            .flex_col()
            .items_center()
            .gap_3()
            .child(div().flex().gap_4().children(quick))
            .child(
                div()
                    .w_full()
                    .max_w(px(640.0))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px_4()
                            .py_2p5()
                            .rounded_full()
                            .bg(rgba((theme.text << 8) | 0x14))
                            .text_size(px(14.5))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    // Writing holds the post.
                                    this.hold(true);
                                    cx.stop_propagation();
                                }),
                            )
                            .child(if recording {
                                div()
                                    .text_color(rgb(theme.muted))
                                    .child("Recording… press the microphone to send")
                                    .into_any_element()
                            } else {
                                self.reply.clone().into_any_element()
                            }),
                    )
                    .child(
                        Button::new("status-voice")
                            .icon(Icon::Mic)
                            .variant(if recording {
                                ButtonVariant::Primary
                            } else {
                                ButtonVariant::Ghost
                            })
                            .round()
                            .tooltip(if recording {
                                "Send voice reply"
                            } else {
                                "Record a voice reply"
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_recording(window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }
}
