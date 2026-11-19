//! The composer's tray: emoji and stickers in one panel, as on the phone,
//! with a tab for each. It floats above the messages from the composer's
//! emoji button.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, SharedString, deferred, div, px, relative, rgb, svg};
use natizap_ui::motion::rise_in;
use natizap_ui::theme::{ActiveTheme as _, Theme};
use natizap_ui::{Icon, Tooltip};

use super::ConversationView;
use crate::emoji;

/// Height of the strip of tabs under the tray's content.
const TABS: f32 = 44.0;

/// What the tray shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum TrayTab {
    #[default]
    Emoji,
    Stickers,
}

impl TrayTab {
    fn label(self) -> &'static str {
        match self {
            Self::Emoji => "Emoji",
            Self::Stickers => "Stickers",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::Emoji => Icon::Emoji,
            Self::Stickers => Icon::Sticker,
        }
    }
}

/// Whether the tray is open, and the tab it shows, kept while it is
/// closed so it opens where the user left it.
#[derive(Debug, Default)]
pub(super) struct Tray {
    open: bool,
    tab: TrayTab,
}

impl Tray {
    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    pub(super) fn close(&mut self) {
        self.open = false;
    }

    pub(super) fn toggle(&mut self) {
        self.open = !self.open;
    }
}

/// The glass panel the emoji picker sits in, wherever it is shown.
pub(super) fn frame(panel: Div, theme: Theme) -> Div {
    panel
        .w(px(emoji::WIDTH))
        .rounded_xl()
        .overflow_hidden()
        .bg(rgb(theme.panel))
        .map(|surface| theme.glass(surface))
}

impl ConversationView {
    /// The tray, floating above the composer while open.
    pub(super) fn tray_popover(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let theme = *cx.theme();
        if !self.tray.open {
            return None;
        }
        // Without the stickers the app hands over there is only one tab.
        let tab = if self.sticker_picker.is_some() {
            self.tray.tab
        } else {
            TrayTab::Emoji
        };
        let content: AnyElement = match (tab, &self.sticker_picker) {
            (TrayTab::Stickers, Some(stickers)) => stickers.clone().into_any_element(),
            _ => self.emoji_picker.clone().into_any_element(),
        };
        let tabs = self.sticker_picker.is_some().then(move || {
            div()
                .h(px(TABS))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .gap_1()
                .border_t_1()
                .border_color(gpui::rgba(theme.edge))
                .child(Self::tab_button(TrayTab::Emoji, tab, cx))
                .child(Self::tab_button(TrayTab::Stickers, tab, cx))
        });
        Some(deferred(
            div()
                .absolute()
                .bottom(relative(1.0))
                .left_4()
                .mb_2()
                .occlude()
                .child(rise_in(
                    "tray-in",
                    div()
                        .h(px(emoji::HEIGHT + TABS))
                        .flex()
                        .flex_col()
                        .map(|tray| frame(tray, theme))
                        .child(div().flex_1().min_h_0().child(content))
                        .children(tabs),
                    8.0,
                )),
        ))
    }

    fn tab_button(tab: TrayTab, shown: TrayTab, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let current = tab == shown;
        div()
            .id(SharedString::from(format!("tray-{}", tab.label())))
            .h(px(30.0))
            .px_3()
            .flex()
            .items_center()
            .gap_1p5()
            .rounded_full()
            .cursor_pointer()
            .text_size(px(13.0))
            .text_color(rgb(if current { theme.text } else { theme.muted }))
            .when(current, |button| button.bg(rgb(theme.raised)))
            .hover(|button| button.text_color(rgb(theme.text)))
            .tooltip(Tooltip::text(tab.label()))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.tray.tab = tab;
                cx.notify();
            }))
            .child(
                svg()
                    .path(tab.icon().path())
                    .size(px(16.0))
                    .text_color(rgb(if current { theme.text } else { theme.muted })),
            )
            .child(tab.label())
    }
}
