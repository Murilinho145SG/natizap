//! How the other end of a call is heard here: as loud as the user set
//! them, kept for the next call with them, or not at all while the user
//! mutes them for themselves. The other end is never told.

use std::time::Duration;

use gpui::Context;

use super::{Call, CallDesk};
use crate::volume::{Listening, Loudness};

/// How long a loudness rests before it is kept: a drag sets dozens.
const SETTLE: Duration = Duration::from_millis(500);

impl CallDesk {
    /// How loud the call's other end plays.
    #[must_use]
    pub fn peer_loudness(&self) -> Loudness {
        self.call
            .as_ref()
            .and_then(|call| self.choices.loudness.get(call.peer.as_str()))
            .copied()
            .unwrap_or_default()
    }

    /// Plays the call's other end as loud as `loudness`, now and in the
    /// next calls with them.
    pub fn set_peer_loudness(&mut self, loudness: Loudness, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_ref() else {
            return;
        };
        let person = call.peer.as_str().to_owned();
        if loudness == Loudness::SENT {
            self.choices.loudness.remove(&person);
        } else {
            self.choices.loudness.insert(person, loudness);
        }
        self.apply_loudness();
        // Kept once the drag rests.
        self.keeping = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETTLE).await;
            let _ = this.update(cx, |desk, cx| desk.choices.write(desk.settings.clone(), cx));
        }));
        cx.notify();
    }

    /// Mutes the call's other end for the user alone, or hears them again.
    pub fn toggle_peer_muted(&mut self, cx: &mut Context<Self>) {
        if let Some(call) = self.call.as_mut() {
            call.listening = call.listening.toggled();
            self.apply_loudness();
            cx.notify();
        }
    }

    /// Plays the other end as the user set them, once the call has sound.
    pub(super) fn apply_loudness(&self) {
        let loudness = self.peer_loudness();
        if let Some(Call {
            audio: Some(audio),
            listening,
            ..
        }) = self.call.as_ref()
        {
            audio.set_volume(match listening {
                Listening::Heard => loudness.gain(),
                Listening::Muted => 0.0,
            });
        }
    }
}
