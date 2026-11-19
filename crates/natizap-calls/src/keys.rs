//! The call's controls on the keyboard, wherever natizap has focus: the
//! keys meeting apps use, so hands that know them need not learn new ones.

use gpui::{Action, App, Entity, actions};

use crate::desk::CallDesk;

actions!(
    call,
    [
        /// Silences the microphone, or lets it be heard again.
        ToggleMute,
        /// Turns the camera on or off.
        ToggleCamera,
        /// Ends the call under way.
        HangUp
    ]
);

/// A call control's keys, as GPUI binds them: `secondary` is Cmd on macOS
/// and Ctrl elsewhere.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Shortcut(&'static str);

pub(crate) const MUTE: Shortcut = Shortcut("secondary-shift-m");
pub(crate) const CAMERA: Shortcut = Shortcut("secondary-shift-o");
pub(crate) const HANG_UP: Shortcut = Shortcut("secondary-shift-h");

impl Shortcut {
    /// The keys as a tooltip spells them: `Ctrl+Shift+M`, or `⌘⇧M` on a
    /// Mac.
    pub(crate) fn label(self) -> String {
        let mac = cfg!(target_os = "macos");
        let parts: Vec<String> = self
            .0
            .split('-')
            .map(|part| match (part, mac) {
                ("secondary", true) => "⌘".to_owned(),
                ("secondary", false) => "Ctrl".to_owned(),
                ("shift", true) => "⇧".to_owned(),
                ("shift", false) => "Shift".to_owned(),
                (key, _) => key.to_uppercase(),
            })
            .collect();
        parts.join(if mac { "" } else { "+" })
    }

    /// `tip`, with the keys that do the same after it.
    pub(crate) fn tip(self, tip: &str) -> String {
        format!("{tip} ({})", self.label())
    }
}

/// The call controls' key bindings, for the app to bind.
#[must_use]
pub fn call_keys() -> Vec<(&'static str, Box<dyn Action>)> {
    vec![
        (MUTE.0, Box::new(ToggleMute)),
        (CAMERA.0, Box::new(ToggleCamera)),
        (HANG_UP.0, Box::new(HangUp)),
    ]
}

impl CallDesk {
    /// Answers the call controls' keys in every window, for `desk`.
    pub fn answer_keys(desk: &Entity<Self>, cx: &mut App) {
        let muting = desk.clone();
        cx.on_action(move |_: &ToggleMute, cx| muting.update(cx, Self::toggle_muted));
        let filming = desk.clone();
        cx.on_action(move |_: &ToggleCamera, cx| filming.update(cx, Self::toggle_camera));
        let ending = desk.clone();
        cx.on_action(move |_: &HangUp, cx| ending.update(cx, Self::hang_up));
    }
}

#[cfg(test)]
mod tests {
    use super::{HANG_UP, MUTE};

    #[test]
    fn tooltips_spell_the_keys_for_the_system() {
        if cfg!(target_os = "macos") {
            assert_eq!(MUTE.label(), "⌘⇧M");
        } else {
            assert_eq!(MUTE.label(), "Ctrl+Shift+M");
            assert_eq!(HANG_UP.tip("Hang up"), "Hang up (Ctrl+Shift+H)");
        }
    }
}
