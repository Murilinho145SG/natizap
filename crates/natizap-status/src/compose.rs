//! Posting a status: text on a colour, or a photo or video with a caption.

use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{
    Context, Div, Entity, EventEmitter, Focusable as _, FontWeight, PathPromptOptions,
    SharedString, Window, div, px, rgb, rgba,
};
use natizap_ui::theme::{ActiveTheme as _, STATUS_BACKGROUNDS, Theme};
use natizap_ui::{Button, ButtonVariant, Icon, TextInput, dialog, separator};

use crate::book::StatusBook;

/// The user closed the composer, after posting or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComposerClosed;

/// What is being posted.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Draft {
    Text { background: usize },
    File(PathBuf),
}

/// The dialog that posts a status.
pub struct StatusComposer {
    book: Entity<StatusBook>,
    draft: Draft,
    /// The text, or a file's caption.
    words: Entity<TextInput>,
    posting: bool,
    error: Option<SharedString>,
}

impl std::fmt::Debug for StatusComposer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatusComposer")
            .field("draft", &self.draft)
            .finish_non_exhaustive()
    }
}

impl EventEmitter<ComposerClosed> for StatusComposer {}

impl StatusComposer {
    /// Posts through `book`, starting on a text status.
    pub fn new(book: Entity<StatusBook>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let words = cx.new(|cx| TextInput::new("Type a status", cx).wrapping());
        window.focus(&words.focus_handle(cx), cx);
        cx.observe(&words, |_, _, cx| cx.notify()).detach();
        Self {
            book,
            draft: Draft::Text { background: 0 },
            words,
            posting: false,
            error: None,
        }
    }

    fn pick_file(cx: &mut Context<Self>) {
        let picking = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut paths))) = picking.await else {
                return;
            };
            let Some(path) = paths.pop() else {
                return;
            };
            let _ = this.update(cx, |composer, cx| {
                composer.draft = Draft::File(path);
                composer.error = None;
                cx.notify();
            });
        })
        .detach();
    }

    fn post(&mut self, cx: &mut Context<Self>) {
        let words = self.words.read(cx).text().trim().to_owned();
        let posting = match self.draft.clone() {
            Draft::Text { background } => {
                if words.is_empty() {
                    self.error = Some("Write something to post".into());
                    cx.notify();
                    return;
                }
                let color = STATUS_BACKGROUNDS[background % STATUS_BACKGROUNDS.len()];
                self.book
                    .update(cx, |book, cx| book.post_text(words, color, cx))
            }
            Draft::File(path) => {
                let caption = (!words.is_empty()).then_some(words);
                self.book
                    .update(cx, |book, cx| book.post_file(path, caption, cx))
            }
        };
        self.posting = true;
        self.error = None;
        cx.spawn(async move |this, cx| {
            let posted = posting.await;
            let _ = this.update(cx, |composer, cx| {
                composer.posting = false;
                match posted {
                    Ok(()) => cx.emit(ComposerClosed),
                    Err(error) => {
                        tracing::warn!(%error, "could not post a status");
                        composer.error = Some(error.to_string().into());
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// The colours a text status can be posted on, the chosen one ringed.
    fn swatches(&self, theme: Theme, cx: &mut Context<Self>) -> Option<Div> {
        matches!(self.draft, Draft::Text { .. }).then(|| {
            div()
                .flex()
                .gap_2()
                .children(STATUS_BACKGROUNDS.iter().enumerate().map(|(index, color)| {
                    let chosen = self.draft == Draft::Text { background: index };
                    div()
                        .id(("status-color", index))
                        .size(px(24.0))
                        .rounded_full()
                        .bg(rgba(color.rotate_left(8)))
                        .border_2()
                        .border_color(if chosen {
                            rgb(theme.text)
                        } else {
                            gpui::transparent_black().into()
                        })
                        .cursor_pointer()
                        .on_click(cx.listener(move |composer, _, _, cx| {
                            composer.draft = Draft::Text { background: index };
                            cx.notify();
                        }))
                }))
        })
    }

    fn preview(&self, theme: Theme) -> Div {
        match &self.draft {
            Draft::Text { background } => div()
                .h(px(220.0))
                .rounded_lg()
                .bg(rgba(STATUS_BACKGROUNDS[*background].rotate_left(8)))
                .p_5()
                .flex()
                .items_center()
                .justify_center()
                .text_center()
                .text_size(px(20.0))
                .font_weight(FontWeight::MEDIUM)
                .child(self.words.clone()),
            Draft::File(path) => {
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(13.5))
                            .text_color(rgb(theme.muted))
                            .child(path.file_name().map_or_else(String::new, |name| {
                                name.to_string_lossy().into_owned()
                            })),
                    )
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(theme.line))
                            .child(self.words.clone()),
                    )
            }
        }
    }
}

impl Render for StatusComposer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let swatches = self.swatches(theme, cx);
        let panel = div()
            .w(px(440.0))
            .p_5()
            .flex()
            .flex_col()
            .gap_4()
            .rounded_xl()
            .bg(rgb(theme.panel))
            .map(|surface| theme.glass(surface))
            .text_color(rgb(theme.text))
            .occlude()
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Post a status"),
                    )
                    .child(
                        Button::new("close-composer")
                            .icon(Icon::Close)
                            .variant(ButtonVariant::Ghost)
                            .round()
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(ComposerClosed))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("compose-text")
                            .label("Text")
                            .variant(ButtonVariant::Secondary)
                            .selected(matches!(self.draft, Draft::Text { .. }))
                            .on_click(cx.listener(|composer, _, _, cx| {
                                composer.draft = Draft::Text { background: 0 };
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("compose-file")
                            .icon(Icon::Camera)
                            .label("Photo or video")
                            .variant(ButtonVariant::Secondary)
                            .selected(matches!(self.draft, Draft::File(_)))
                            .on_click(cx.listener(|_, _, _, cx| Self::pick_file(cx))),
                    ),
            )
            .child(self.preview(theme))
            .children(swatches)
            .child(separator(theme))
            .children(self.error.clone().map(|error| {
                div()
                    .text_size(px(12.5))
                    .text_color(rgb(theme.alert))
                    .child(error)
            }))
            .child(
                div().flex().justify_end().child(
                    Button::new("post")
                        .label(if self.posting { "Posting…" } else { "Post" })
                        .on_click(cx.listener(|composer, _, _, cx| {
                            if !composer.posting {
                                composer.post(cx);
                            }
                        })),
                ),
            );
        let view = cx.weak_entity();
        dialog(
            window,
            panel,
            move |_, cx| {
                let _ = view.update(cx, |_, cx| cx.emit(ComposerClosed));
            },
            theme,
        )
    }
}
