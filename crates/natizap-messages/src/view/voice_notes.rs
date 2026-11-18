//! Playing voice notes and audio files in their bubbles: play and pause,
//! seeking, speed, and redrawing the bubble while one plays.

use std::time::Duration;

use gpui::{Context, SharedString};

use super::ConversationView;
use crate::items::MediaView;
use natizap_playback::Speed;

/// How often a playing note's bubble is redrawn: often enough for the
/// waveform and clock, far less than every frame.
const VOICE_TICK: Duration = Duration::from_millis(50);

impl ConversationView {
    /// Plays or pauses a voice note or audio file in its bubble.
    pub(crate) fn toggle_voice(&mut self, media: &MediaView, cx: &mut Context<Self>) {
        let id = SharedString::from(media.fetch.message_id.clone());
        if self.voices.toggle(&id, media.file.as_deref()) {
            let fetch = media.fetch.clone();
            self.log
                .update(cx, |log, cx| log.open_media(fetch, None, cx));
        }
        self.keep_ticking(cx);
    }

    /// Jumps to `fraction` of the playing note.
    pub(crate) fn seek_voice(&mut self, message_id: &str, fraction: f32, cx: &mut Context<Self>) {
        self.voices.seek(message_id, fraction);
        cx.notify();
    }

    /// Plays voice notes at `speed` from now on.
    pub(crate) fn set_voice_speed(&mut self, speed: Speed, cx: &mut Context<Self>) {
        self.voices.set_speed(speed);
        cx.notify();
    }

    /// Opens or closes the speed menu of `message_id`'s bubble.
    pub(crate) fn toggle_speed_menu(&mut self, message_id: &SharedString, cx: &mut Context<Self>) {
        self.voices.toggle_speed_menu(message_id);
        cx.notify();
    }

    /// Redraws the playing note until it pauses, stops or ends.
    pub(super) fn keep_ticking(&mut self, cx: &mut Context<Self>) {
        cx.notify();
        if self.ticking || !self.voices.is_running() {
            return;
        }
        self.ticking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(VOICE_TICK).await;
                let running = this.update(cx, |view, cx| {
                    cx.notify();
                    view.ticking = view.voices.is_running();
                    view.ticking
                });
                if !matches!(running, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }
}
