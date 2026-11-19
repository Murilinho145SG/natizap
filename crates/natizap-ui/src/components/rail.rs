//! The narrow column on the window's left edge that switches between the
//! app's places, with the account's own avatar at its foot.

use gpui::prelude::*;
use gpui::{AnyElement, App, Div, ElementId, Stateful, Window, div, px, rgb, rgba};

use crate::theme::{ActiveTheme as _, Theme};

/// Width the rail takes.
pub const RAIL_WIDTH: f32 = 64.0;
/// Size of the avatar at the rail's foot.
pub const RAIL_AVATAR: f32 = 34.0;

/// The rail: places at the top, settings and the account at the bottom.
#[derive(IntoElement, Default)]
pub struct Rail {
    top: Vec<AnyElement>,
    bottom: Vec<AnyElement>,
}

impl std::fmt::Debug for Rail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rail")
            .field("top", &self.top.len())
            .field("bottom", &self.bottom.len())
            .finish()
    }
}

impl Rail {
    /// An empty rail.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `item`, usually a round ghost `Button`, under the ones at the
    /// top.
    #[must_use]
    pub fn top(mut self, item: impl IntoElement) -> Self {
        self.top.push(item.into_any_element());
        self
    }

    /// Adds `item` under the ones at the bottom.
    #[must_use]
    pub fn bottom(mut self, item: impl IntoElement) -> Self {
        self.bottom.push(item.into_any_element());
        self
    }
}

impl RenderOnce for Rail {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.theme();
        let stack = || div().flex().flex_col().items_center().gap_2();
        div()
            .w(px(RAIL_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .justify_between()
            .py_3()
            .bg(rgb(theme.panel))
            .border_r_1()
            .border_color(rgb(theme.line))
            .child(stack().children(self.top))
            .child(stack().children(self.bottom))
    }
}

/// `avatar` (from [`avatar`](crate::avatar) at [`RAIL_AVATAR`]) as a rail
/// item, ringed while its place is `selected`. Add `on_click` to it.
pub fn rail_avatar(
    id: impl Into<ElementId>,
    avatar: Div,
    selected: bool,
    theme: Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .p(px(3.0))
        .rounded_full()
        .border_2()
        .border_color(if selected {
            rgb(theme.text)
        } else {
            gpui::transparent_black().into()
        })
        .cursor_pointer()
        .hover(|item| item.bg(rgba(theme.hover)))
        .child(avatar)
}
