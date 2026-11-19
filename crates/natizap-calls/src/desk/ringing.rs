//! Calls ringing here: they come and go, each with a system notification,
//! and the ringtone or the ringback plays while something rings.

use gpui::{Context, SystemNotification};
use natizap_protocol::{Ring, RingChange};

use super::{CallDesk, CallRang, Phase};
use crate::ringtone::Ringing;

impl CallDesk {
    pub(super) fn ring(&mut self, change: RingChange, cx: &mut Context<Self>) {
        match change {
            RingChange::Rang(ring) => {
                if self.rings.iter().any(|other| other.id == ring.id) {
                    return;
                }
                let title = self.title(&ring.caller, cx);
                cx.show_system_notification(SystemNotification {
                    tag: ring.caller.as_str().to_owned().into(),
                    title,
                    body: if ring.video {
                        "Incoming video call".into()
                    } else {
                        "Incoming voice call".into()
                    },
                    actions: Vec::new(),
                });
                cx.emit(CallRang {
                    caller: ring.caller.clone(),
                });
                self.rings.push(ring);
            }
            RingChange::Stopped(id) => {
                let Some(ring) = self.rings.iter().find(|ring| ring.id == id).cloned() else {
                    return;
                };
                Self::dismiss_notification(&ring, cx);
                self.rings.retain(|ring| ring.id != id);
            }
        }
        self.update_ringer(cx);
        cx.notify();
    }

    pub(super) fn dismiss_notification(ring: &Ring, cx: &gpui::App) {
        // Dismissing goes on even when the task is dropped.
        drop(cx.dismiss_system_notification(ring.caller.as_str()));
    }

    /// Rings when a call comes in with none under way, or while the other
    /// phone rings; is quiet otherwise.
    pub(super) fn update_ringer(&mut self, cx: &Context<Self>) {
        let wanted = match self.call.as_ref().map(|call| &call.phase) {
            None if !self.rings.is_empty() => Some(Ringing::Incoming(self.choices.ringtone)),
            Some(Phase::Ringing) => Some(Ringing::Ringback),
            _ => None,
        };
        if self.ringer.as_ref().map(|(tone, _)| *tone) == wanted {
            return;
        }
        self.ringer = wanted.map(|tone| (tone, tone.ring(cx)));
    }
}
