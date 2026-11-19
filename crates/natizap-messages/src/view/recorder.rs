//! Recording a voice note from the composer: the send button turns into a
//! microphone while there is nothing to send.

use std::time::Duration;

use gpui::prelude::*;
use gpui::{Context, Div, SharedString, div, px, rgb, svg};
use natizap_playback::{RecordError, Recorder};
use natizap_ui::theme::ActiveTheme as _;
use natizap_ui::{Button, ButtonSize, ButtonVariant, Icon};

use super::ConversationView;

/// How often the recording's clock is redrawn.
const TICK: Duration = Duration::from_millis(250);
/// How long the reason the microphone would not record stays shown.
const PROBLEM_SHOWN: Duration = Duration::from_secs(8);

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
            Err(error) => {
                tracing::warn!(%error, "could not record");
                let problem: SharedString = match error {
                    RecordError::NoMicrophone => "No microphone was found.".into(),
                    // The usual cause on Windows and macOS: the system
                    // keeps apps from the microphone until allowed.
                    _ => "The microphone could not be opened. Check that natizap may use it \
                          in your system's privacy settings."
                        .into(),
                };
                self.mic_problem = Some(problem.clone());
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(PROBLEM_SHOWN).await;
                    let _ = this.update(cx, |this, cx| {
                        if this.mic_problem.as_ref() == Some(&problem) {
                            this.mic_problem = None;
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
        }
        cx.notify();
    }

    /// Why the microphone would not record, while it is shown.
    pub(super) fn mic_problem(&self, cx: &mut Context<Self>) -> Option<Div> {
        let theme = *cx.theme();
        let problem = self.mic_problem.clone()?;
        Some(
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_size(px(13.0))
                .text_color(rgb(theme.alert))
                .child(
                    svg()
                        .path(Icon::MicOff.path())
                        .size(px(14.0))
                        .flex_none()
                        .text_color(rgb(theme.alert)),
                )
                .child(problem),
        )
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
        let theme = *cx.theme();
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
                    .child(div().size(px(10.0)).rounded_full().bg(rgb(theme.alert)))
                    .child(div().text_color(rgb(theme.text)).child(format!(
                        "{}:{:02}",
                        seconds / 60,
                        seconds % 60
                    )))
                    .child(
                        div()
                            .text_size(px(13.0))
                            .text_color(rgb(theme.muted))
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
