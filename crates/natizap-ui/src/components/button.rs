//! Buttons: a label, an icon, or both, in one of four variants.

use gpui::prelude::*;
use gpui::{
    App, ClickEvent, ElementId, FontWeight, Pixels, SharedString, Window, div, px, rgb, svg,
};

use crate::Icon;
use crate::components::tooltip::Tooltip;
use crate::theme::{ACCENT, ACCENT_INK, ALERT, LINE, MUTED, RAISED, TEXT};

/// What a button is for, which sets its colors.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    /// The one action a place is about, e.g. send.
    #[default]
    Primary,
    /// A plain action beside a primary one.
    Secondary,
    /// A quiet control that shows its surface only on hover.
    Ghost,
    /// An action that destroys something.
    Danger,
}

/// How big a button is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ButtonSize {
    /// 28 pixels tall, for controls inside content.
    Small,
    /// 36 pixels tall.
    #[default]
    Medium,
    /// 44 pixels tall, for a screen's main action.
    Large,
}

impl ButtonSize {
    fn height(self) -> Pixels {
        px(match self {
            Self::Small => 28.0,
            Self::Medium => 36.0,
            Self::Large => 44.0,
        })
    }

    fn icon(self) -> Pixels {
        px(match self {
            Self::Small => 15.0,
            Self::Medium => 16.0,
            Self::Large => 20.0,
        })
    }
}

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A clickable control. Build with [`Button::new`] and the builder methods.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<Icon>,
    variant: ButtonVariant,
    size: ButtonSize,
    round: bool,
    tooltip: Option<SharedString>,
    on_click: Option<ClickHandler>,
}

impl std::fmt::Debug for Button {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Button")
            .field("id", &self.id)
            .field("label", &self.label)
            .finish_non_exhaustive()
    }
}

impl Button {
    /// A medium primary button with nothing on it yet.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            label: None,
            icon: None,
            variant: ButtonVariant::default(),
            size: ButtonSize::default(),
            round: false,
            tooltip: None,
            on_click: None,
        }
    }

    /// Text on the button.
    #[must_use]
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// An icon, before the label if there is one.
    #[must_use]
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Its colors.
    #[must_use]
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Its height.
    #[must_use]
    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    /// Fully rounded: a circle for an icon alone, a pill otherwise.
    #[must_use]
    pub fn round(mut self) -> Self {
        self.round = true;
        self
    }

    /// Names the button on hover; icon-only buttons need one.
    #[must_use]
    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    /// What a click does.
    #[must_use]
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let (background, ink, hover) = match self.variant {
            ButtonVariant::Primary => (Some(ACCENT), ACCENT_INK, None),
            ButtonVariant::Secondary => (Some(RAISED), TEXT, Some(LINE)),
            ButtonVariant::Ghost => (None, MUTED, Some(RAISED)),
            ButtonVariant::Danger => (Some(ALERT), TEXT, None),
        };
        let height = self.size.height();
        let icon_only = self.label.is_none();
        div()
            .id(self.id)
            .h(height)
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .gap_2()
            .when(icon_only, |button| button.w(height))
            .when(!icon_only, |button| {
                button.px(height * 0.4).text_size(height * 0.38)
            })
            .map(|button| {
                if self.round {
                    button.rounded_full()
                } else {
                    button.rounded_lg()
                }
            })
            .when_some(background, |button, background| button.bg(rgb(background)))
            .text_color(rgb(ink))
            .font_weight(FontWeight::SEMIBOLD)
            .cursor_pointer()
            .map(|button| match hover {
                Some(hover) => {
                    button.hover(move |style| style.bg(rgb(hover)).text_color(rgb(TEXT)))
                }
                None => button.hover(|style| style.opacity(0.9)),
            })
            .when_some(self.tooltip, |button, text| {
                button.tooltip(Tooltip::text(text))
            })
            .when_some(self.on_click, |button, handler| button.on_click(handler))
            .when_some(self.icon, |button, icon| {
                button.child(
                    svg()
                        .path(icon.path())
                        .size(self.size.icon())
                        .text_color(rgb(ink)),
                )
            })
            .when_some(self.label, ParentElement::child)
    }
}
