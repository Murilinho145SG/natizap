//! Who is typing or recording, just above the composer, below the last
//! message rather than over it: their
//! faces, their first names when the user likes them shown, and dots that
//! breathe, or a microphone while someone records.

use gpui::prelude::*;
use gpui::{Animation, AnimationExt as _, Context, Div, SharedString, div, px, rgb, svg};
use natizap_ui::motion::{BREATH, swell};
use natizap_ui::theme::{ActiveTheme as _, Theme};
use natizap_ui::{Icon, avatar};

use super::ConversationView;
use crate::log::{Typer, TypingLook};
use natizap_protocol::Activity;

/// How many faces show; the names say the rest.
const MOST_FACES: usize = 3;
const FACE: f32 = 20.0;

impl ConversationView {
    /// The note of who is typing, while anyone is.
    pub(super) fn typing_note(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let theme = *cx.theme();
        let log = self.log.read(cx);
        let typers = log.typers(cx);
        if typers.is_empty() {
            return None;
        }
        let look = log.typing_look();
        let recording = typers
            .iter()
            .all(|typer| typer.activity == Activity::Recording);
        let faces: Vec<_> = typers
            .iter()
            .take(MOST_FACES)
            .map(|typer| {
                let picture = self
                    .pictures
                    .update(cx, |pictures, cx| pictures.picture(&typer.id, cx));
                (typer.clone(), picture)
            })
            .collect();
        Some(
            div()
                .flex_none()
                .self_start()
                .ml_4()
                .mb_1()
                .px_2()
                .py_1()
                .flex()
                .items_center()
                .gap_2()
                .rounded_full()
                .bg(rgb(theme.panel))
                .map(|surface| theme.glass(surface))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .children(faces.into_iter().enumerate().map(
                            |(place, (typer, picture))| {
                                div()
                                    .rounded_full()
                                    .border_2()
                                    .border_color(rgb(theme.panel))
                                    .when(place > 0, |face| face.ml(px(-FACE / 3.0)))
                                    .child(avatar(
                                        typer.id.as_str(),
                                        &typer.name,
                                        picture,
                                        px(FACE),
                                    ))
                            },
                        )),
                )
                .when(look == TypingLook::NamesAndFaces, |note| {
                    note.child(
                        div()
                            .text_size(px(12.5))
                            .text_color(rgb(theme.muted))
                            .child(names(&typers)),
                    )
                })
                .child(if recording {
                    microphone(theme)
                } else {
                    dots(theme)
                }),
        )
    }
}

/// "Ana", "Ana and Bruno", or "Ana and 2 others".
fn names(typers: &[Typer]) -> SharedString {
    match typers {
        [] => SharedString::default(),
        [one] => one.name.clone(),
        [one, two] => format!("{} and {}", one.name, two.name).into(),
        [one, rest @ ..] => format!("{} and {} others", one.name, rest.len()).into(),
    }
}

/// Three dots that brighten one after another.
fn dots(theme: Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1()
        .children((0..3u8).map(|place| {
            let offset = f32::from(place) / 3.0;
            div()
                .size(px(5.0))
                .rounded_full()
                .bg(rgb(theme.muted))
                .with_animation(
                    ("typing-dot", u64::from(place)),
                    Animation::new(BREATH).repeat(),
                    move |dot, delta| {
                        // Bright as each dot's turn comes round, faint between.
                        let lit = swell((delta + 1.0 - offset) % 1.0);
                        dot.opacity(0.3 + 0.7 * lit)
                    },
                )
        }))
}

/// A microphone that pulses while someone records.
fn microphone(theme: Theme) -> Div {
    div().child(
        svg()
            .path(Icon::Mic.path())
            .size(px(13.0))
            .text_color(rgb(theme.muted))
            .with_animation(
                "typing-recording",
                Animation::new(BREATH).repeat(),
                |mic, delta| mic.opacity(0.4 + 0.6 * swell(delta)),
            ),
    )
}

#[cfg(test)]
mod tests {
    use natizap_protocol::ChatId;

    use super::*;

    fn typer(name: &str) -> Typer {
        Typer {
            id: ChatId::new("1@lid"),
            name: name.to_owned().into(),
            activity: Activity::Typing,
        }
    }

    #[test]
    fn names_one_two_or_the_first_and_how_many_more() {
        assert_eq!(names(&[typer("Ana")]).as_ref(), "Ana");
        assert_eq!(
            names(&[typer("Ana"), typer("Bruno")]).as_ref(),
            "Ana and Bruno"
        );
        assert_eq!(
            names(&[typer("Ana"), typer("Bruno"), typer("Caio")]).as_ref(),
            "Ana and 2 others"
        );
    }
}
