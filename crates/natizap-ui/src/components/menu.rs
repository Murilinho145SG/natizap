//! A list of actions that floats over the window, e.g. on a right click.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    App, Div, MouseButton, Pixels, Point, SharedString, Window, anchored, deferred, div, px, rgb,
    svg,
};

use crate::Icon;
use crate::components::separator::separator;
use crate::theme::{ALERT, MUTED, PANEL, RAISED, TEXT, glass};

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

enum Entry {
    Action {
        icon: Icon,
        label: SharedString,
        danger: bool,
        on_select: Handler,
    },
    Separator,
}

/// A menu of actions. Build it with [`Menu::new`], then show it where the
/// user clicked with [`Menu::at`], or inline (as the gallery does).
#[derive(IntoElement)]
pub struct Menu {
    entries: Vec<Entry>,
    on_dismiss: Option<Handler>,
}

impl std::fmt::Debug for Menu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Menu")
            .field("entries", &self.entries.len())
            .finish_non_exhaustive()
    }
}

impl Default for Menu {
    fn default() -> Self {
        Self::new()
    }
}

impl Menu {
    /// An empty menu.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            on_dismiss: None,
        }
    }

    /// An action; picking it runs `on_select`.
    #[must_use]
    pub fn action(
        mut self,
        icon: Icon,
        label: impl Into<SharedString>,
        on_select: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.entries.push(Entry::Action {
            icon,
            label: label.into(),
            danger: false,
            on_select: Rc::new(on_select),
        });
        self
    }

    /// An action that destroys something, set apart in the alert color.
    #[must_use]
    pub fn danger(
        mut self,
        icon: Icon,
        label: impl Into<SharedString>,
        on_select: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.entries.push(Entry::Action {
            icon,
            label: label.into(),
            danger: true,
            on_select: Rc::new(on_select),
        });
        self
    }

    /// A hairline between groups of actions.
    #[must_use]
    pub fn separator(mut self) -> Self {
        self.entries.push(Entry::Separator);
        self
    }

    /// Runs `on_dismiss` when the user presses outside the menu.
    #[must_use]
    pub fn on_dismiss(mut self, on_dismiss: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Some(Rc::new(on_dismiss));
        self
    }

    /// The menu floating at `position` in the window, kept inside the
    /// window when the click was near an edge.
    #[must_use]
    pub fn at(self, position: Point<Pixels>) -> impl IntoElement {
        deferred(
            anchored()
                .position(position)
                .snap_to_window_with_margin(px(8.0))
                .child(self),
        )
        .with_priority(1)
    }
}

impl RenderOnce for Menu {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let on_dismiss = self.on_dismiss;
        div()
            .id("menu")
            .w(px(224.0))
            .py_1()
            .flex()
            .flex_col()
            .rounded_xl()
            .bg(rgb(PANEL))
            .map(glass)
            .occlude()
            .when_some(on_dismiss, |menu, on_dismiss| {
                menu.on_mouse_down_out(move |_, window, cx| on_dismiss(window, cx))
            })
            .children(
                self.entries
                    .into_iter()
                    .enumerate()
                    .map(|(index, entry)| match entry {
                        Entry::Separator => div().mx_3().my_1().child(separator()),
                        Entry::Action {
                            icon,
                            label,
                            danger,
                            on_select,
                        } => action_row(index, icon, label, danger, on_select),
                    }),
            )
    }
}

fn action_row(
    index: usize,
    icon: Icon,
    label: SharedString,
    danger: bool,
    on_select: Handler,
) -> Div {
    let (ink, glyph) = if danger {
        (ALERT, ALERT)
    } else {
        (TEXT, MUTED)
    };
    div().px_1().child(
        div()
            .id(("menu-action", index))
            .h(px(34.0))
            .px_3()
            .flex()
            .items_center()
            .gap_3()
            .rounded_md()
            .text_size(px(14.0))
            .text_color(rgb(ink))
            .cursor_pointer()
            .hover(|row| row.bg(rgb(RAISED)))
            // On press, so the action runs even when the press also counts
            // as a click outside another floating element.
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                cx.stop_propagation();
                on_select(window, cx);
            })
            .child(
                svg()
                    .path(icon.path())
                    .size(px(16.0))
                    .flex_none()
                    .text_color(rgb(glyph)),
            )
            .child(label),
    )
}
