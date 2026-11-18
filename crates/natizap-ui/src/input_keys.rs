//! The keys a [`TextInput`](crate::TextInput) answers to, and the actions
//! they fire.

use std::rc::Rc;

use gpui::{Action, App, KeyBinding, KeyBindingContextPredicate, actions};

actions!(
    text_input,
    [
        Backspace,
        Delete,
        DeleteWordLeft,
        DeleteWordRight,
        Left,
        Right,
        WordLeft,
        WordRight,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectWordLeft,
        SelectWordRight,
        SelectAll,
        Home,
        End,
        Paste,
        Cut,
        Copy,
        Submit,
        Newline,
    ]
);

pub(crate) const KEY_CONTEXT: &str = "TextInput";

/// Registers the keyboard shortcuts every [`TextInput`] responds to. Call
/// once at startup.
pub fn bind_text_input_keys(cx: &mut App) {
    let context = match KeyBindingContextPredicate::parse(KEY_CONTEXT) {
        Ok(context) => Some(Rc::new(context)),
        Err(error) => {
            tracing::error!(%error, "invalid text input key context");
            return;
        }
    };
    // Words are Alt on macOS and Ctrl elsewhere, unlike `secondary`.
    let word = if cfg!(target_os = "macos") {
        "alt"
    } else {
        "ctrl"
    };
    let word_bindings: [(String, Box<dyn Action>); 6] = [
        (format!("{word}-backspace"), Box::new(DeleteWordLeft)),
        (format!("{word}-delete"), Box::new(DeleteWordRight)),
        (format!("{word}-left"), Box::new(WordLeft)),
        (format!("{word}-right"), Box::new(WordRight)),
        (format!("{word}-shift-left"), Box::new(SelectWordLeft)),
        (format!("{word}-shift-right"), Box::new(SelectWordRight)),
    ];
    let bindings: [(&str, Box<dyn Action>); 16] = [
        ("backspace", Box::new(Backspace)),
        ("delete", Box::new(Delete)),
        ("left", Box::new(Left)),
        ("right", Box::new(Right)),
        ("up", Box::new(Up)),
        ("down", Box::new(Down)),
        ("shift-left", Box::new(SelectLeft)),
        ("shift-right", Box::new(SelectRight)),
        // `secondary` is Cmd on macOS and Ctrl elsewhere.
        ("secondary-a", Box::new(SelectAll)),
        ("secondary-v", Box::new(Paste)),
        ("secondary-c", Box::new(Copy)),
        ("secondary-x", Box::new(Cut)),
        ("home", Box::new(Home)),
        ("end", Box::new(End)),
        ("enter", Box::new(Submit)),
        ("shift-enter", Box::new(Newline)),
    ];
    let bindings: Vec<KeyBinding> = bindings
        .into_iter()
        .map(|(keys, action)| (keys.to_owned(), action))
        .chain(word_bindings)
        .filter_map(|(keys, action)| {
            KeyBinding::load(&keys, action, context.clone(), false, None, cx)
                .inspect_err(|error| tracing::error!(%error, keys, "invalid key binding"))
                .ok()
        })
        .collect();
    cx.bind_keys(bindings);
}
