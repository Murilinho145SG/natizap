//! Every component in every state, in a window of its own: where the look
//! is reviewed, and where a component lives until a feature uses it.

use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, Entity, FontWeight, ScrollHandle, SharedString, Window, div, px, rgb,
};

use crate::components::alert::{Alert, AlertTone};
use crate::components::alert_dialog::AlertDialog;
use crate::components::badge::{BadgeTone, badge, count_label, verified_seal};
use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::components::code_block::code_block;
use crate::components::color_picker::ColorPicker;
use crate::components::menu::Menu;
use crate::components::progress::progress;
use crate::components::rail::{RAIL_AVATAR, Rail, rail_avatar};
use crate::components::scrollbar::scrollbar;
use crate::components::separator::separator;
use crate::components::settings_window::{SettingsClosed, SettingsPage, SettingsWindow};
use crate::components::skeleton::skeleton;
use crate::components::slider::Slider;
use crate::components::switch::Switch;
use crate::theme::{ActiveTheme as _, DISPLAY_FONT, Theme, base_font};
use crate::{Icon, avatar, window_frame};

/// The components window's content.
#[derive(Debug, Default)]
pub struct Gallery {
    dialog_open: bool,
    /// The settings window shown, while it is.
    settings: Option<Entity<SettingsWindow>>,
    /// The colour picker on show, made at the first draw.
    picker: Option<Entity<ColorPicker>>,
    switch_on: bool,
    /// Where the demo slider stands.
    slider_value: f32,
    scroll: ScrollHandle,
}

impl Render for Gallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let picker = self
            .picker
            .get_or_insert_with(|| cx.new(|_| ColorPicker::new(theme.accent, false)))
            .clone();
        let title = div()
            .font_family(DISPLAY_FONT)
            .font_weight(FontWeight::BOLD)
            .text_size(px(17.0))
            .child("Components");
        let sections = div()
            .id("gallery")
            .size_full()
            .overflow_y_scroll()
            .bg(rgb(theme.pane))
            .p_8()
            .flex()
            .flex_col()
            .gap_10()
            .child(section("Button", buttons(), theme))
            .child(section("Badge", badges(theme), theme))
            .child(section(
                "Tooltip",
                row().child(
                    Button::new("tooltip-demo")
                        .icon(Icon::Reply)
                        .variant(ButtonVariant::Secondary)
                        .round()
                        .tooltip("Reply"),
                ),
                theme,
            ))
            .child(section("Switch", self.switches(cx), theme))
            .child(section("Slider", self.sliders(cx), theme))
            .child(section("Code block", code_sample(theme), theme))
            .child(section("Avatar", avatars(), theme))
            .child(section("Rail", rail_demo(theme), theme))
            .child(section(
                "Separator",
                div().w(px(320.0)).child(separator(theme)),
                theme,
            ))
            .child(section("Skeleton", skeleton_row(theme), theme))
            .child(section(
                "Progress",
                div()
                    .w(px(320.0))
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(progress(0.0, theme))
                    .child(progress(0.35, theme))
                    .child(progress(1.0, theme)),
                theme,
            ))
            .child(section("Scrollbar", self.scrolled(window, cx), theme))
            .child(section("Alert", alerts(), theme))
            .child(section(
                "Menu",
                Menu::new()
                    .action(Icon::Reply, "Reply", |_, _| {})
                    .action(Icon::Copy, "Copy", |_, _| {})
                    .separator()
                    .danger(Icon::Trash, "Delete", |_, _| {}),
                theme,
            ))
            .child(section("Dialogs", Self::dialog_buttons(cx), theme))
            .child(section("Color picker", row().child(picker), theme))
            .children(self.settings.clone())
            .when(self.dialog_open, |sections| {
                let gallery = cx.weak_entity();
                let close = move |_: &mut Window, cx: &mut gpui::App| {
                    let _ = gallery.update(cx, |gallery, cx| {
                        gallery.dialog_open = false;
                        cx.notify();
                    });
                };
                sections.child(
                    AlertDialog::new("Delete message?", close.clone())
                        .description("It is deleted for everyone in this chat.")
                        .action("Cancel", ButtonVariant::Ghost, close.clone())
                        .action("Delete", ButtonVariant::Danger, close),
                )
            });
        div()
            .size_full()
            .font(base_font())
            .text_color(rgb(theme.text))
            .child(window_frame(window, title, sections, theme))
    }
}

impl Gallery {
    /// Buttons that open an alert dialog and a settings window.
    fn dialog_buttons(cx: &mut Context<Self>) -> Div {
        row()
            .child(
                Button::new("open-dialog")
                    .label("Delete message")
                    .variant(ButtonVariant::Secondary)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.dialog_open = true;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("open-settings")
                    .label("Open settings")
                    .variant(ButtonVariant::Secondary)
                    .on_click(cx.listener(|this, _, _, cx| this.open_settings(cx))),
            )
    }

    /// A settings window of two pages that say which they are.
    fn open_settings(&mut self, cx: &mut Context<Self>) {
        let pages = vec![
            SettingsPage::new(
                Icon::Palette,
                "Appearance",
                cx.new(|_| DemoPage("How natizap looks")),
            ),
            SettingsPage::new(
                Icon::Bell,
                "Notifications",
                cx.new(|_| DemoPage("How natizap tells you")),
            ),
        ];
        let settings = cx.new(|_| SettingsWindow::new(pages));
        cx.subscribe(&settings, |this, _, _: &SettingsClosed, cx| {
            this.settings = None;
            cx.notify();
        })
        .detach();
        self.settings = Some(settings);
        cx.notify();
    }

    /// A switch that flips, beside one on and one off.
    fn switches(&self, cx: &mut Context<Self>) -> Div {
        row()
            .child(Switch::new("switch-demo", self.switch_on).on_toggle({
                let gallery = cx.weak_entity();
                move |on, _, cx| {
                    let _ = gallery.update(cx, |gallery, cx| {
                        gallery.switch_on = on;
                        cx.notify();
                    });
                }
            }))
            .child(Switch::new("switch-on", true))
            .child(Switch::new("switch-off", false))
    }

    /// A slider that moves, with a mark where nothing changes.
    fn sliders(&self, cx: &mut Context<Self>) -> Div {
        row().child(
            div().w(px(240.0)).child(
                Slider::new("slider-demo", self.slider_value)
                    .mark(0.5)
                    .on_change({
                        let gallery = cx.weak_entity();
                        move |value, _, cx| {
                            let _ = gallery.update(cx, |gallery, cx| {
                                gallery.slider_value = value;
                                cx.notify();
                            });
                        }
                    }),
            ),
        )
    }

    /// A short list scrolled inside a box, with its scrollbar.
    fn scrolled(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = *cx.theme();
        div()
            .relative()
            .w(px(320.0))
            .h(px(160.0))
            .rounded_lg()
            .bg(rgb(theme.panel))
            .child(
                div()
                    .id("scrollbar-demo")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .children((1..=16).map(|n| div().child(format!("Row {n}")))),
            )
            .child(scrollbar("scrollbar-demo-bar", &self.scroll, window, cx))
    }
}

fn section(name: &'static str, body: impl IntoElement, theme: Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(
            div()
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(theme.muted))
                .child(name),
        )
        .child(body)
}

fn row() -> Div {
    div().flex().flex_wrap().items_center().gap_3()
}

fn buttons() -> Div {
    let variants = [
        ("Primary", ButtonVariant::Primary),
        ("Secondary", ButtonVariant::Secondary),
        ("Ghost", ButtonVariant::Ghost),
        ("Danger", ButtonVariant::Danger),
    ];
    let sizes = [
        ("small", ButtonSize::Small),
        ("medium", ButtonSize::Medium),
        ("large", ButtonSize::Large),
    ];
    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(row().children(variants.map(|(name, variant)| {
            Button::new(SharedString::from(format!("variant-{name}")))
                .label(name)
                .variant(variant)
        })))
        .child(row().children(sizes.map(|(name, size)| {
            Button::new(SharedString::from(format!("size-{name}")))
                .label(name)
                .icon(Icon::Send)
                .size(size)
        })))
        .child(row().children(sizes.map(|(name, size)| {
            Button::new(SharedString::from(format!("round-{name}")))
                .icon(Icon::Send)
                .size(size)
                .round()
                .tooltip(format!("Round, {name}"))
        })))
        .child(
            row()
                .child(
                    Button::new("ghost-off")
                        .icon(Icon::Emoji)
                        .variant(ButtonVariant::Ghost)
                        .round()
                        .tooltip("Ghost"),
                )
                .child(
                    Button::new("ghost-selected")
                        .icon(Icon::Emoji)
                        .variant(ButtonVariant::Ghost)
                        .round()
                        .selected(true)
                        .tooltip("Ghost, selected"),
                ),
        )
}

fn badges(theme: Theme) -> Div {
    row()
        .child(badge("3", BadgeTone::Accent, theme))
        .child(badge(count_label(1500), BadgeTone::Accent, theme))
        .child(badge("12", BadgeTone::Muted, theme))
        .child(badge("", BadgeTone::Accent, theme))
        .child(verified_seal(18.0, theme))
}

fn avatars() -> Div {
    row()
        .child(avatar("a", "Ana", None, px(28.0)))
        .child(avatar("b", "Bruno", None, px(40.0)))
        .child(avatar("c", "Carla", None, px(56.0)))
}

fn rail_demo(theme: Theme) -> Div {
    div().h(px(260.0)).child(
        Rail::new()
            .top(
                Button::new("rail-chats")
                    .icon(Icon::Chats)
                    .variant(ButtonVariant::Ghost)
                    .round()
                    .selected(true)
                    .tooltip("Chats"),
            )
            .bottom(
                Button::new("rail-settings")
                    .icon(Icon::Settings)
                    .variant(ButtonVariant::Ghost)
                    .round()
                    .tooltip("Settings"),
            )
            .bottom(rail_avatar(
                "rail-me",
                avatar("me", "Dani", None, px(RAIL_AVATAR)),
                false,
                theme,
            )),
    )
}

fn skeleton_row(theme: Theme) -> AnyElement {
    div()
        .w(px(320.0))
        .flex()
        .items_center()
        .gap_3()
        .child(skeleton(
            "avatar",
            div().size(px(48.0)).rounded_full(),
            theme,
        ))
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap_2()
                .child(skeleton(
                    "title",
                    div().w(px(140.0)).h(px(12.0)).rounded_md(),
                    theme,
                ))
                .child(skeleton(
                    "preview",
                    div().w_full().h(px(10.0)).rounded_md(),
                    theme,
                )),
        )
        .into_any_element()
}

fn alerts() -> Div {
    div()
        .w(px(420.0))
        .flex()
        .flex_col()
        .gap_3()
        .child(Alert::new(AlertTone::Info, "Linked").description("Your chats are syncing."))
        .child(
            Alert::new(AlertTone::Warning, "Reconnecting…")
                .description("The connection dropped. natizap keeps trying."),
        )
        .child(Alert::new(AlertTone::Error, "Not sent").description("Tap to try again."))
}

fn code_sample(theme: Theme) -> Div {
    div().w(px(420.0)).child(code_block(
        "// Greets everyone in the chat\nfn greet(names: &[String]) -> usize {\n    for name in names {\n        println!(\"Hello, {name}!\");\n    }\n    names.len() * 2\n}", theme))
}

/// A settings page that only says what it is about.
struct DemoPage(&'static str);

impl Render for DemoPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_5()
            .py_3()
            .text_color(rgb(cx.theme().muted))
            .child(self.0)
    }
}
