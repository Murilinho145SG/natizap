//! Recording a voice note from the composer: the send button turns into a
//! microphone while there is nothing to send.

use std::time::Duration;

use gpui::prelude::*;
use gpui::{Context, Div, div, px, rgb};
use natizap_playback::Recorder;
use natizap_ui::theme::{ALERT, MUTED, TEXT};
use natizap_ui::{Button, ButtonSize, ButtonVariant, Icon};

use super::ConversationView;

/// How often the recording's clock is redrawn.
const TICK: Duration = Duration::from_millis(250);

impl ConversationView {
    /// Send while there is text, else a microphone that starts recording.
    pub(super) fn send_or_record(&self, cx: &mut Context<Self>) -> Button {
        if self.composer.read(cx).text().trim().is_empty() {
            Button::new("record")
                .icon(Icon::Mic)
                .size(ButtonSize::Large)
                .round()
                .tooltip("Record a voice message")
                .on_click(cx.listener(|this, _, _, cx| this.start_recording(cx)))
        } else {
            Button::new("send")
                .icon(Icon::Send)
                .size(ButtonSize::Large)
                .round()
                .tooltip("Send")
                .on_click(cx.listener(|this, _, window, cx| this.send(window, cx)))
        }
    }

    /// Whether a voice note is being recorded.
    pub(super) fn recording(&self) -> bool {
        self.recorder.is_some()
    }

    fn start_recording(&mut self, cx: &mut Context<Self>) {
        match Recorder::start() {
            Ok(recorder) => {
                self.recorder = Some(recorder);
                cx.spawn(async move |this, cx| {
                    loop {
                        cx.background_executor().timer(TICK).await;
                        let recording = this.update(cx, |this, cx| {
                            cx.notify();
                            this.recording()
                        });
                        if !recording.unwrap_or(false) {
                            break;
                        }
                    }
                })
                .detach();
            }
            Err(error) => tracing::warn!(%error, "could not record"),
        }
        cx.notify();
    }

    fn finish_recording(&mut self, send: bool, cx: &mut Context<Self>) {
        let Some(recorder) = self.recorder.take() else {
            return;
        };
        if send {
            let heard = recorder.stop();
            let reply = self.replying.take().map(|replying| replying.reply);
            self.log
                .update(cx, |log, cx| log.send_voice(heard, reply, cx));
        }
        cx.notify();
    }

    /// The composer while recording: throw it away, how long it is, send.
    pub(super) fn recording_bar(&self, cx: &mut Context<Self>) -> Div {
        let seconds = self
            .recorder
            .as_ref()
            .map_or(0, |recorder| recorder.elapsed().as_secs());
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(
                Button::new("discard-recording")
                    .icon(Icon::Trash)
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Large)
                    .round()
                    .tooltip("Discard")
                    .on_click(cx.listener(|this, _, _, cx| this.finish_recording(false, cx))),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().size(px(10.0)).rounded_full().bg(rgb(ALERT)))
                    .child(div().text_color(rgb(TEXT)).child(format!(
                        "{}:{:02}",
                        seconds / 60,
                        seconds % 60
                    )))
                    .child(
                        div()
                            .text_size(px(13.0))
                            .text_color(rgb(MUTED))
                            .child("Recording"),
                    ),
            )
            .child(
                Button::new("send-recording")
                    .icon(Icon::Send)
                    .size(ButtonSize::Large)
                    .round()
                    .tooltip("Send")
                    .on_click(cx.listener(|this, _, _, cx| this.finish_recording(true, cx))),
            )
    }
}
