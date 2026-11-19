//! The calls settings page: which ringtone calls coming in ring with. The
//! choice is kept in a small text file of `key=value` lines, so it reads
//! back across versions.

use std::path::{Path, PathBuf};

use gpui::prelude::*;
use gpui::{App, Context, Entity, FontWeight, SharedString, Window, div, px, rgb, svg};
use natizap_ui::Icon;
use natizap_ui::theme::ActiveTheme as _;

use crate::desk::CallDesk;
use crate::ringtone::Ringtone;

/// The ringtone the settings file at `path` names, or the default when it
/// names none or cannot be read.
pub(crate) fn read(path: &Path) -> Ringtone {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text),
        Err(error) => {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(%error, "could not read the call settings");
            }
            Ringtone::default()
        }
    }
}

/// Keeps `ringtone` in the settings file at `path`, off the UI thread.
pub(crate) fn write(path: PathBuf, ringtone: Ringtone, cx: &App) {
    cx.background_spawn(async move {
        if let Err(error) = std::fs::write(&path, format(ringtone)) {
            tracing::warn!(%error, "could not keep the call settings");
        }
    })
    .detach();
}

/// The ringtone the file's `text` names. Unknown lines, such as ones a
/// newer version wrote, are skipped.
fn parse(text: &str) -> Ringtone {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(key, _)| key.trim() == "ringtone")
        .find_map(|(_, value)| Ringtone::from_key(value.trim()))
        .unwrap_or_default()
}

fn format(ringtone: Ringtone) -> String {
    format!("ringtone={}\n", ringtone.key())
}

/// Changes a [`CallDesk`]'s ringtone: the calls page of the settings
/// window.
#[derive(Debug)]
pub struct CallSettings {
    desk: Entity<CallDesk>,
}

impl CallSettings {
    /// Shows and changes `desk`'s ringtone.
    pub fn new(desk: Entity<CallDesk>, cx: &mut Context<Self>) -> Self {
        cx.observe(&desk, |_, _, cx| cx.notify()).detach();
        Self { desk }
    }

    /// A ringtone to choose; choosing it plays it.
    fn chip(&self, ringtone: Ringtone, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = *cx.theme();
        let selected = self.desk.read(cx).ringtone() == ringtone;
        let tint = if selected { theme.accent } else { theme.text };
        div()
            .id(SharedString::from(format!("ringtone-{}", ringtone.key())))
            .flex()
            .items_center()
            .gap_1p5()
            .px_3()
            .py_1p5()
            .rounded_full()
            .border_1()
            .border_color(rgb(if selected { theme.accent } else { theme.line }))
            .text_size(px(13.0))
            .text_color(rgb(tint))
            .cursor_pointer()
            .hover(|chip| chip.bg(rgb(theme.raised)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.desk
                    .update(cx, |desk, cx| desk.set_ringtone(ringtone, cx));
            }))
            .child(
                svg()
                    .path(Icon::Play.path())
                    .size(px(11.0))
                    .text_color(rgb(tint)),
            )
            .child(ringtone.name())
    }
}

impl Render for CallSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let chips: Vec<_> = Ringtone::ALL
            .into_iter()
            .map(|ringtone| self.chip(ringtone, cx).into_any_element())
            .collect();
        div().flex().flex_col().child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .px_5()
                .py_3()
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(theme.muted))
                        .child("Ringtone"),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .text_color(rgb(theme.muted))
                        .child("What a call coming in rings with. Choosing one plays it."),
                )
                .child(div().flex().flex_wrap().gap_2().children(chips)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{format, parse};
    use crate::ringtone::Ringtone;

    #[test]
    fn reads_back_what_it_wrote() {
        for ringtone in Ringtone::ALL {
            assert_eq!(parse(&format(ringtone)), ringtone);
        }
    }

    #[test]
    fn keeps_the_default_for_what_the_file_lacks_or_garbles() {
        assert_eq!(parse(""), Ringtone::default());
        assert_eq!(parse("ringtone=trumpet\nvolume=11\n"), Ringtone::default());
        assert_eq!(parse("volume=11\nringtone = guitar\n"), Ringtone::Guitar);
    }
}
