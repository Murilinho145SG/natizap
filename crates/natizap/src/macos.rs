//! What a Mac expects of an app beyond its window: a menu bar with Hide and
//! Quit, and their keys.

use gpui::{App, KeyBinding, Menu, MenuItem, actions};

actions!(
    natizap,
    [
        /// Hides natizap's windows, as Cmd-H does in every Mac app.
        Hide,
        /// Ends natizap, tray and all.
        Quit
    ]
);

/// Puts natizap's menu in the menu bar.
pub(crate) fn add_menu(cx: &mut App) {
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.bind_keys([
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("cmd-q", Quit, None),
    ]);
    cx.set_menus([Menu::new("natizap").items([
        MenuItem::action("Hide natizap", Hide),
        MenuItem::separator(),
        MenuItem::action("Quit natizap", Quit),
    ])]);
}
