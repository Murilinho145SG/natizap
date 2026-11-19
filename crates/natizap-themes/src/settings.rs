//! The appearance section of the settings window: a card per theme, the
//! one in use ringed, and the editor that makes a theme of one's own.

use gpui::prelude::*;
use gpui::{Context, Entity, FontWeight, SharedString, Subscription, Window, div, px, rgb};
use natizap_ui::theme::{ActiveTheme as _, Theme};
use natizap_ui::{Button, ButtonSize, ButtonVariant, Icon};

use crate::book::{Choice, ThemeBook};
use crate::editor::{Draft, actions};

/// Chooses, makes and changes themes.
pub struct ThemeSettings {
    book: Entity<ThemeBook>,
    /// The theme being made, while the editor is open.
    pub(crate) draft: Option<Draft>,
    /// Following the editor's fields while it is open.
    fields: Vec<Subscription>,
}

impl std::fmt::Debug for ThemeSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThemeSettings").finish_non_exhaustive()
    }
}

impl ThemeSettings {
    /// Shows `book`'s themes.
    pub fn new(book: Entity<ThemeBook>, cx: &mut Context<Self>) -> Self {
        cx.observe(&book, |_, _, cx| cx.notify()).detach();
        Self {
            book,
            draft: None,
            fields: Vec::new(),
        }
    }

    /// Opens the editor on the theme in use: a copy of a native one, or
    /// the user's own to change.
    fn customize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let book = self.book.read(cx);
        let chosen = book.chosen().clone();
        let custom = book
            .choices()
            .into_iter()
            .find(|choice| choice.name == chosen)
            .is_some_and(|choice| choice.custom);
        let base = book.base_of(&chosen);
        let (editing, name) = if custom {
            (Some(chosen.clone()), chosen.to_string())
        } else {
            (None, format!("{chosen} copy"))
        };
        let theme = *cx.theme();
        let draft = Draft::new(base, editing, &name, theme, window, cx);
        self.fields = draft
            .inputs()
            .map(|input| {
                cx.observe(input, |this, _, cx| {
                    if let Some(draft) = &mut this.draft {
                        draft.update(&this.book, cx);
                    }
                })
            })
            .collect();
        self.draft = Some(draft);
        cx.notify();
    }

    fn close_editor(&mut self, cx: &mut Context<Self>) {
        self.draft = None;
        self.fields.clear();
        cx.notify();
    }

    fn card(
        choice: &Choice,
        chosen: bool,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let shown = choice.theme;
        let name = choice.name.clone();
        let group = SharedString::from(format!("theme-card-{}", choice.name));
        let deleted = choice.name.clone();
        div()
            .id(SharedString::from(format!("theme-{}", choice.name)))
            .group(group.clone())
            .relative()
            .w(px(84.0))
            .flex()
            .flex_col()
            .gap_1p5()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.book.update(cx, |book, cx| book.choose(&name, cx));
            }))
            .child(
                div()
                    .h(px(52.0))
                    .rounded_lg()
                    .border_2()
                    .border_color(rgb(if chosen { theme.accent } else { theme.line }))
                    .bg(rgb(shown.pane))
                    .overflow_hidden()
                    .flex()
                    .child(div().w(px(22.0)).h_full().bg(rgb(shown.panel)))
                    .child(
                        div()
                            .flex_1()
                            .p_1p5()
                            .flex()
                            .flex_col()
                            .justify_end()
                            .items_end()
                            .gap_1()
                            .child(
                                div()
                                    .w(px(28.0))
                                    .h(px(8.0))
                                    .rounded_full()
                                    .bg(rgb(shown.raised)),
                            )
                            .child(
                                div()
                                    .w(px(22.0))
                                    .h(px(8.0))
                                    .rounded_full()
                                    .bg(rgb(shown.accent)),
                            ),
                    ),
            )
            .child(
                div()
                    .truncate()
                    .text_size(px(12.5))
                    .text_color(rgb(if chosen { theme.text } else { theme.muted }))
                    .child(choice.name.clone()),
            )
            // The user's own themes can go: a bin over the card's corner,
            // under the pointer.
            .when(choice.custom, |card| {
                card.child(
                    div()
                        .absolute()
                        .top_1()
                        .right_1()
                        .invisible()
                        .group_hover(group, Styled::visible)
                        .child(
                            Button::new(SharedString::from(format!("delete-theme-{deleted}")))
                                .icon(Icon::Trash)
                                .variant(ButtonVariant::Secondary)
                                .size(ButtonSize::Small)
                                .round()
                                .tooltip("Delete theme")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    // The card behind would choose it.
                                    cx.stop_propagation();
                                    let deleted = deleted.clone();
                                    this.book.update(cx, |book, cx| book.delete(&deleted, cx));
                                })),
                        ),
                )
            })
    }
}

impl Render for ThemeSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let book = self.book.read(cx);
        let chosen = book.chosen().clone();
        let choices = book.choices();
        let section = div().flex().flex_col().gap_3().px_5().py_3().child(
            div()
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(theme.muted))
                .child("Theme"),
        );
        if let Some(draft) = &self.draft {
            let editing = draft.editing().is_some();
            return section.child(draft.render(theme, cx)).child(actions(
                editing,
                |this: &mut Self, cx| {
                    if let Some(name) = this
                        .draft
                        .as_ref()
                        .and_then(|draft| draft.editing().cloned())
                    {
                        this.book.update(cx, |book, cx| book.delete(&name, cx));
                    }
                    this.close_editor(cx);
                },
                |this: &mut Self, cx| {
                    this.book.update(cx, |book, cx| book.restore(cx));
                    this.close_editor(cx);
                },
                |this: &mut Self, cx| {
                    if let Some(draft) = &this.draft {
                        draft.save(&this.book, cx);
                    }
                    this.close_editor(cx);
                },
                cx,
            ));
        }
        let cards: Vec<_> = choices
            .iter()
            .map(|choice| Self::card(choice, choice.name == chosen, theme, cx).into_any_element())
            .collect();
        section
            .child(div().flex().flex_wrap().gap_3().children(cards))
            .child(
                div().child(
                    Button::new("customize-theme")
                        .icon(Icon::Edit)
                        .label("Customize")
                        .variant(ButtonVariant::Secondary)
                        .on_click(cx.listener(|this, _, window, cx| this.customize(window, cx))),
                ),
            )
    }
}
