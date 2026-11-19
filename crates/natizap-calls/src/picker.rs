//! natizap's own dialog asking what to share in a call, where the desktop
//! has none (Windows and macOS): every screen and window with a small
//! picture of each, on two tabs. Clicking one shares it at once.

use gpui::prelude::*;
use gpui::{Context, Entity, FontWeight, Window, div, px, rgb, rgba};
use natizap_playback::SourceKind;
use natizap_ui::theme::{ActiveTheme as _, Theme};
use natizap_ui::{Button, ButtonVariant, dialog};

use crate::desk::{CallDesk, ShareChoice, ShareChoices};
use crate::screen::Painted;

/// A source's small picture's size.
const THUMBNAIL: (f32, f32) = (200.0, 112.0);

/// The dialog asking what to share, while the desk asks.
pub struct SharePicker {
    desk: Entity<CallDesk>,
    tab: SourceKind,
    /// What this window painted of each source's picture.
    painted: Vec<Painted>,
}

impl std::fmt::Debug for SharePicker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharePicker")
            .field("tab", &self.tab)
            .finish_non_exhaustive()
    }
}

impl SharePicker {
    /// The dialog for `desk`'s call, shown while it asks what to share.
    pub fn new(desk: Entity<CallDesk>, cx: &mut Context<Self>) -> Self {
        cx.observe(&desk, |_, _, cx| cx.notify()).detach();
        Self {
            desk,
            tab: SourceKind::Screen,
            painted: Vec::new(),
        }
    }

    /// Lets go of every picture painted, once the dialog closes.
    fn release(&mut self, window: &mut Window) {
        for painted in &mut self.painted {
            painted.paint(None, px(0.0), window);
        }
        self.painted.clear();
        self.tab = SourceKind::Screen;
    }

    /// The screens or windows on the tab shown, as cards to click.
    fn sources(
        &mut self,
        all: &[ShareChoice],
        theme: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        self.painted.resize_with(all.len(), Painted::default);
        let mut cards = Vec::new();
        for (index, choice) in all.iter().enumerate() {
            let shown = choice.source.kind() == self.tab;
            let thumbnail = choice.thumbnail.clone().filter(|_| shown);
            let painted = self.painted[index].paint(thumbnail, px(6.0), window);
            if shown {
                cards.push(card(index, choice, painted, theme, cx));
            }
        }
        if cards.is_empty() {
            let none = match self.tab {
                SourceKind::Screen => "No screens to share",
                SourceKind::Window => "No windows to share",
            };
            return note(none, theme).into_any_element();
        }
        div()
            .flex()
            .flex_wrap()
            .gap_2()
            .children(cards)
            .into_any_element()
    }

    /// The tab of screens, or of windows.
    fn tab(&self, kind: SourceKind, label: &'static str, cx: &mut Context<Self>) -> Button {
        let id = match kind {
            SourceKind::Screen => "share-tab-screens",
            SourceKind::Window => "share-tab-windows",
        };
        Button::new(id)
            .label(label)
            .variant(ButtonVariant::Ghost)
            .selected(self.tab == kind)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.tab = kind;
                cx.notify();
            }))
    }
}

impl Render for SharePicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let Some(choices) = self.desk.read(cx).share_choices() else {
            self.release(window);
            return div().into_any_element();
        };
        let body = match choices {
            ShareChoices::Finding => {
                note("Looking for screens and windows…", theme).into_any_element()
            }
            ShareChoices::Found(all) => self.sources(&all, theme, window, cx),
        };
        let desk = self.desk.clone();
        let cancel = move |_: &mut Window, cx: &mut gpui::App| {
            desk.update(cx, CallDesk::cancel_sharing);
        };
        let cancelling = cancel.clone();
        let panel = div()
            .id("share-picker")
            .w(px(680.0))
            .max_w(window.viewport_size().width - px(32.0))
            .p_5()
            .flex()
            .flex_col()
            .gap_4()
            .rounded_xl()
            .bg(rgb(theme.panel))
            .map(|surface| theme.glass(surface))
            .occlude()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(16.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Share your screen"),
                    )
                    .child(self.tab(SourceKind::Screen, "Screens", cx))
                    .child(self.tab(SourceKind::Window, "Windows", cx)),
            )
            .child(
                div()
                    .id("share-picker-sources")
                    .h(px(388.0))
                    .max_h(window.viewport_size().height - px(200.0))
                    .overflow_y_scroll()
                    .child(body),
            )
            .child(
                div().flex().justify_end().child(
                    Button::new("share-cancel")
                        .label("Cancel")
                        .variant(ButtonVariant::Secondary)
                        .on_click(move |_, window, cx| cancelling(window, cx)),
                ),
            );
        dialog(window, panel, cancel, theme).into_any_element()
    }
}

/// A screen or window to click to share it.
fn card(
    index: usize,
    choice: &ShareChoice,
    painted: gpui::Div,
    theme: Theme,
    cx: &mut Context<SharePicker>,
) -> impl IntoElement + use<> {
    let source = choice.source;
    div()
        .id(("share-source", index))
        .w(px(THUMBNAIL.0 + 12.0))
        .p(px(6.0))
        .flex()
        .flex_col()
        .gap_1()
        .rounded_lg()
        .cursor_pointer()
        .hover(|card| card.bg(rgba(theme.hover)))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.desk.update(cx, |desk, cx| desk.share_from(source, cx));
        }))
        .child(
            div()
                .relative()
                .w(px(THUMBNAIL.0))
                .h(px(THUMBNAIL.1))
                .rounded_md()
                .bg(rgb(theme.deep))
                .child(painted),
        )
        .child(div().pt_1().truncate().text_sm().child(choice.name.clone()))
        .children(choice.app.clone().map(|app| {
            div()
                .truncate()
                .text_xs()
                .text_color(rgb(theme.muted))
                .child(app)
        }))
}

/// A line in place of the sources: while they are looked for, or when
/// there are none.
fn note(text: &'static str, theme: Theme) -> impl IntoElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_color(rgb(theme.muted))
        .child(text)
}
