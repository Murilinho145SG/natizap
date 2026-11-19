//! Making a theme: a name, then each colour as a swatch and its hex, shown
//! live on the whole app while it is edited.

use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    Anchor, Context, Entity, FontWeight, SharedString, Window, anchored, deferred, div, px, rgb,
    rgba,
};
use natizap_ui::theme::Theme;
use natizap_ui::{
    Button, ButtonVariant, ColorPicked, ColorPicker, ColorPickerDismissed, TextInput,
};

use crate::book::ThemeBook;
use crate::settings::ThemeSettings;
use crate::tokens::{self, GROUPS, Token};

/// A theme being made, from a native one or the user's own.
pub(crate) struct Draft {
    /// The native theme it starts from.
    base: SharedString,
    /// The user's theme it changes, if it changes one.
    editing: Option<SharedString>,
    theme: Theme,
    name: Entity<TextInput>,
    /// Each colour's hex field, in the editor's order.
    fields: Vec<(Token, Entity<TextInput>)>,
    /// The colour whose picker is open, by key, and the picker.
    picking: Option<(&'static str, Entity<ColorPicker>)>,
    /// The colour whose picker a press outside just closed, and when.
    dismissed: Option<(&'static str, Instant)>,
}

impl Draft {
    /// A draft of `theme`, starting from `base`, changing the user's theme
    /// `editing` if given, else a new one named `name`.
    pub(crate) fn new<V: 'static>(
        base: SharedString,
        editing: Option<SharedString>,
        name: &str,
        theme: Theme,
        window: &mut Window,
        cx: &mut Context<V>,
    ) -> Self {
        let field = |text: String, window: &mut Window, cx: &mut Context<V>| {
            let input = cx.new(|cx| TextInput::new("#000000", cx));
            input.update(cx, |input, cx| input.insert(&text, window, cx));
            input
        };
        let name = field(name.to_owned(), window, cx);
        let fields = tokens::all()
            .map(|token| (*token, field(token.write((token.get)(&theme)), window, cx)))
            .collect();
        Self {
            base,
            editing,
            theme,
            name,
            fields,
            picking: None,
            dismissed: None,
        }
    }

    /// Opens the picker on the colour `key`, or closes it if it is open
    /// there. The picker writes what it picks into the colour's field.
    pub(crate) fn toggle_picker(
        &mut self,
        key: &'static str,
        window: &mut Window,
        cx: &mut Context<ThemeSettings>,
    ) {
        // A press on the swatch closes its picker as a press outside it
        // before the click comes: the click must not open it again.
        let just_closed = self
            .dismissed
            .take()
            .is_some_and(|(closed, at)| closed == key && at.elapsed() < Duration::from_millis(400));
        if just_closed || self.picking.as_ref().is_some_and(|(open, _)| *open == key) {
            self.picking = None;
            return;
        }
        let Some((token, input)) = self.fields.iter().find(|(token, _)| token.key == key) else {
            return;
        };
        let (token, input) = (*token, input.clone());
        let picker = cx.new(|_| ColorPicker::new((token.get)(&self.theme), token.alpha));
        cx.subscribe_in(
            &picker,
            window,
            move |_, _, ColorPicked(color), window, cx| {
                let hex = token.write(*color);
                input.update(cx, |input, cx| {
                    input.clear(window, cx);
                    input.insert(&hex, window, cx);
                });
            },
        )
        .detach();
        cx.subscribe(&picker, move |this, _, _: &ColorPickerDismissed, cx| {
            if let Some(draft) = &mut this.draft {
                draft.picking = None;
                draft.dismissed = Some((key, Instant::now()));
                cx.notify();
            }
        })
        .detach();
        self.picking = Some((key, picker));
    }

    /// The hex fields, to follow their changes.
    pub(crate) fn inputs(&self) -> impl Iterator<Item = &Entity<TextInput>> {
        self.fields.iter().map(|(_, input)| input)
    }

    /// Takes in every hex that reads, and shows the result on the app.
    pub(crate) fn update(&mut self, book: &Entity<ThemeBook>, cx: &mut gpui::App) {
        for (token, input) in &self.fields {
            if let Some(color) = token.read(input.read(cx).text()) {
                (token.set)(&mut self.theme, color);
            }
        }
        let theme = self.theme;
        book.update(cx, |_, cx| ThemeBook::preview(theme, cx));
    }

    /// Keeps the draft as the user's theme and draws with it.
    pub(crate) fn save(&self, book: &Entity<ThemeBook>, cx: &mut gpui::App) {
        let name = self.name.read(cx).text().to_owned();
        let (base, theme) = (self.base.clone(), self.theme);
        book.update(cx, |book, cx| book.save(&name, &base, theme, cx));
    }

    /// The user's theme being changed, if any.
    pub(crate) fn editing(&self) -> Option<&SharedString> {
        self.editing.as_ref()
    }

    /// The editor: the name, then each group of colours; a swatch opens
    /// the picker under its colour.
    pub(crate) fn render(
        &self,
        theme: Theme,
        cx: &mut Context<ThemeSettings>,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id("theme-editor")
            .max_h(px(360.0))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_3()
            .child(row("Name", None, self.name.clone(), theme, cx))
            .children(GROUPS.iter().map(|(group, members)| {
                div()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .child(
                        div()
                            .pt_1()
                            .text_size(px(12.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(theme.muted))
                            .child(*group),
                    )
                    .children(members.iter().filter_map(|member| {
                        let (token, input) = self
                            .fields
                            .iter()
                            .find(|(token, _)| token.key == member.key)?;
                        let color = (token.get)(&self.theme);
                        let swatch = if token.alpha {
                            color
                        } else {
                            (color << 8) | 0xff
                        };
                        let picker = self
                            .picking
                            .as_ref()
                            .filter(|(open, _)| *open == token.key)
                            .map(|(_, picker)| picker.clone());
                        Some(row(
                            token.label,
                            Some(Swatch {
                                key: token.key,
                                color: swatch,
                                picker,
                            }),
                            input.clone(),
                            theme,
                            cx,
                        ))
                    }))
            }))
    }
}

/// The colour beside a field, and its picker while open.
struct Swatch {
    key: &'static str,
    color: u32,
    picker: Option<Entity<ColorPicker>>,
}

/// A labelled field, with the colour it holds beside it; clicking the
/// colour opens its picker, floating beside it over the settings.
fn row(
    label: &'static str,
    swatch: Option<Swatch>,
    input: Entity<TextInput>,
    theme: Theme,
    cx: &mut Context<ThemeSettings>,
) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_3()
        .child(div().flex_1().min_w_0().text_size(px(13.0)).child(label))
        .children(swatch.map(|Swatch { key, color, picker }| {
            div()
                .id(SharedString::from(format!("swatch-{key}")))
                .relative()
                .size(px(22.0))
                .flex_none()
                .rounded_md()
                .border_1()
                .border_color(rgb(theme.line))
                .bg(rgba(color))
                .cursor_pointer()
                .hover(|swatch| swatch.border_color(rgb(theme.text)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(draft) = &mut this.draft {
                        draft.toggle_picker(key, window, cx);
                        cx.notify();
                    }
                }))
                .children(picker.map(|picker| {
                    // Over the settings window, which sits at priority 2.
                    deferred(
                        anchored()
                            .anchor(Anchor::TopRight)
                            .snap_to_window_with_margin(px(8.0))
                            .child(div().mt(px(28.0)).child(picker)),
                    )
                    .with_priority(3)
                }))
        }))
        .child(
            div()
                .w(px(120.0))
                .px_2()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(rgb(theme.line))
                .text_size(px(13.0))
                .child(input),
        )
}

/// The editor's buttons: delete for the user's own theme, cancel, save.
pub(crate) fn actions<V: 'static>(
    editing: bool,
    on_delete: impl Fn(&mut V, &mut Context<V>) + 'static,
    on_cancel: impl Fn(&mut V, &mut Context<V>) + 'static,
    on_save: impl Fn(&mut V, &mut Context<V>) + 'static,
    cx: &mut Context<V>,
) -> gpui::Div {
    div()
        .flex()
        .gap_2()
        .when(editing, |actions| {
            actions.child(
                Button::new("delete-theme")
                    .label("Delete")
                    .variant(ButtonVariant::Danger)
                    .on_click(cx.listener(move |this, _, _, cx| on_delete(this, cx))),
            )
        })
        .child(div().flex_1())
        .child(
            Button::new("cancel-theme")
                .label("Cancel")
                .variant(ButtonVariant::Ghost)
                .on_click(cx.listener(move |this, _, _, cx| on_cancel(this, cx))),
        )
        .child(
            Button::new("save-theme")
                .label("Save")
                .on_click(cx.listener(move |this, _, _, cx| on_save(this, cx))),
        )
}
