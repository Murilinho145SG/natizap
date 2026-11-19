//! The settings window: categories down a sidebar, the chosen one's page
//! beside them. Each feature brings its page; the window only arranges.

use gpui::prelude::*;
use gpui::{AnyView, Context, EventEmitter, FontWeight, SharedString, Window, div, px, rgb, svg};

use crate::assets::Icon;
use crate::components::button::{Button, ButtonVariant};
use crate::components::dialog::dialog;
use crate::theme::{ActiveTheme as _, DISPLAY_FONT, Theme};

/// The user closed the settings.
#[derive(Debug, Clone, Copy)]
pub struct SettingsClosed;

/// One category of settings: its place in the sidebar and its page.
pub struct SettingsPage {
    icon: Icon,
    title: SharedString,
    view: AnyView,
}

impl std::fmt::Debug for SettingsPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsPage")
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

impl SettingsPage {
    /// A category named `title`, marked by `icon`, showing `view`.
    pub fn new(icon: Icon, title: impl Into<SharedString>, view: impl Into<AnyView>) -> Self {
        Self {
            icon,
            title: title.into(),
            view: view.into(),
        }
    }
}

/// Every feature's settings, one category at a time; emits
/// [`SettingsClosed`].
pub struct SettingsWindow {
    pages: Vec<SettingsPage>,
    shown: usize,
}

impl std::fmt::Debug for SettingsWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsWindow")
            .field("shown", &self.shown)
            .finish_non_exhaustive()
    }
}

impl EventEmitter<SettingsClosed> for SettingsWindow {}

impl SettingsWindow {
    /// Shows `pages`, the first one open.
    #[must_use]
    pub fn new(pages: Vec<SettingsPage>) -> Self {
        Self { pages, shown: 0 }
    }
}

impl SettingsWindow {
    /// The categories, the open one lit.
    fn sidebar(&self, theme: Theme, cx: &mut Context<Self>) -> gpui::Div {
        div()
            .w(px(200.0))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap_0p5()
            .p_3()
            .bg(rgb(theme.panel))
            .border_r_1()
            .border_color(rgb(theme.line))
            .child(
                div()
                    .px_2()
                    .pt_1()
                    .pb_3()
                    .font_family(DISPLAY_FONT)
                    .text_size(px(17.0))
                    .font_weight(FontWeight::BOLD)
                    .child("Settings"),
            )
            .children(self.pages.iter().enumerate().map(|(index, page)| {
                let on = index == self.shown;
                div()
                    .id(("settings-page", index))
                    .h(px(34.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2p5()
                    .rounded_lg()
                    .cursor_pointer()
                    .text_size(px(13.5))
                    .text_color(rgb(if on { theme.text } else { theme.muted }))
                    .when(on, |item| {
                        item.bg(rgb(theme.raised)).font_weight(FontWeight::MEDIUM)
                    })
                    .when(!on, |item| {
                        item.hover(|item| item.bg(gpui::rgba(theme.hover)))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.shown = index;
                        cx.notify();
                    }))
                    .child(
                        svg()
                            .path(page.icon.path())
                            .size(px(16.0))
                            .flex_none()
                            .text_color(rgb(if on { theme.accent } else { theme.muted })),
                    )
                    .child(page.title.clone())
            }))
    }

    /// The open category: its title, a close button, and its page.
    fn content(&self, theme: Theme, cx: &mut Context<Self>) -> gpui::Div {
        let page = self.pages.get(self.shown);
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(56.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px_5()
                    .border_b_1()
                    .border_color(rgb(theme.line))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(15.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .children(page.map(|page| page.title.clone())),
                    )
                    .child(
                        Button::new("close-settings")
                            .icon(Icon::Close)
                            .variant(ButtonVariant::Ghost)
                            .round()
                            .tooltip("Close")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsClosed))),
                    ),
            )
            .child(
                div()
                    .id("settings-page-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .py_2()
                    .children(page.map(|page| page.view.clone())),
            )
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let height = (window.viewport_size().height * 0.85).min(px(600.0));
        let sidebar = self.sidebar(theme, cx);
        let content = self.content(theme, cx);
        let panel = div()
            .w(px(720.0))
            .max_w(window.viewport_size().width * 0.92)
            .h(height)
            .flex()
            .rounded_xl()
            .overflow_hidden()
            .bg(rgb(theme.pane))
            .map(|surface| theme.glass(surface))
            .occlude()
            .text_size(px(14.0))
            .text_color(rgb(theme.text))
            .child(sidebar)
            .child(content);
        let settings = cx.weak_entity();
        dialog(
            window,
            panel,
            move |_, cx| {
                let _ = settings.update(cx, |_, cx| cx.emit(SettingsClosed));
            },
            theme,
        )
    }
}
