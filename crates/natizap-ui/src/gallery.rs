//! Every component in every state, in a window of its own: where the look
//! is reviewed, and where a component lives until a feature uses it.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, FontWeight, SharedString, Window, div, px, rgb};

use crate::components::alert::{Alert, AlertTone};
use crate::components::badge::{BadgeTone, badge, count_label};
use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::components::progress::progress;
use crate::components::separator::separator;
use crate::components::skeleton::skeleton;
use crate::theme::{DISPLAY_FONT, MUTED, PANE, TEXT, base_font};
use crate::{Icon, avatar, window_frame};

/// The components window's content.
#[derive(Debug, Default)]
pub struct Gallery;

impl Render for Gallery {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let title = div()
            .font_family(DISPLAY_FONT)
            .font_weight(FontWeight::BOLD)
            .text_size(px(17.0))
            .child("Components");
        let sections = div()
            .id("gallery")
            .size_full()
            .overflow_y_scroll()
            .bg(rgb(PANE))
            .p_8()
            .flex()
            .flex_col()
            .gap_10()
            .child(section("Button", buttons()))
            .child(section("Badge", badges()))
            .child(section(
                "Tooltip",
                row().child(
                    Button::new("tooltip-demo")
                        .icon(Icon::Reply)
                        .variant(ButtonVariant::Secondary)
                        .round()
                        .tooltip("Reply"),
                ),
            ))
            .child(section("Avatar", avatars()))
            .child(section("Separator", div().w(px(320.0)).child(separator())))
            .child(section("Skeleton", skeleton_row()))
            .child(section(
                "Progress",
                div()
                    .w(px(320.0))
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(progress(0.0))
                    .child(progress(0.35))
                    .child(progress(1.0)),
            ))
            .child(section("Alert", alerts()));
        div()
            .size_full()
            .font(base_font())
            .text_color(rgb(TEXT))
            .child(window_frame(window, title, sections))
    }
}

fn section(name: &'static str, body: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(
            div()
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(MUTED))
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

fn badges() -> Div {
    row()
        .child(badge("3", BadgeTone::Accent))
        .child(badge(count_label(1500), BadgeTone::Accent))
        .child(badge("12", BadgeTone::Muted))
        .child(badge("", BadgeTone::Accent))
}

fn avatars() -> Div {
    row()
        .child(avatar("a", "Ana", px(28.0)))
        .child(avatar("b", "Bruno", px(40.0)))
        .child(avatar("c", "Carla", px(56.0)))
}

fn skeleton_row() -> AnyElement {
    div()
        .w(px(320.0))
        .flex()
        .items_center()
        .gap_3()
        .child(skeleton("avatar", div().size(px(48.0)).rounded_full()))
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap_2()
                .child(skeleton(
                    "title",
                    div().w(px(140.0)).h(px(12.0)).rounded_md(),
                ))
                .child(skeleton("preview", div().w_full().h(px(10.0)).rounded_md())),
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
