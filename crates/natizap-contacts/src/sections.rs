//! The profile's sections about the chat rather than the person: its media,
//! starred messages, notifications and disappearing messages.

use gpui::prelude::*;
use gpui::{
    Context, Div, ImageSource, MouseButton, ObjectFit, Pixels, Point, SharedString, div, img, px,
    rgb, rgba, svg,
};
use natizap_protocol::Disappearing;
use natizap_ui::theme::{DEEP, MUTED, RAISED, SCRIM, TEXT};
use natizap_ui::{Icon, Menu, Switch, separator};

use crate::panel::ProfilePanel;

/// A chat's media and starred messages, as the app read them.
#[derive(Default)]
pub struct ChatSummary {
    /// Photos, videos and documents in the chat.
    pub media: usize,
    /// The newest photos and videos.
    pub tiles: Vec<Tile>,
    /// Messages this account starred in the chat.
    pub starred: usize,
}

impl std::fmt::Debug for ChatSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatSummary")
            .field("media", &self.media)
            .field("tiles", &self.tiles.len())
            .field("starred", &self.starred)
            .finish()
    }
}

/// One recent photo or video.
pub struct Tile {
    /// Its picture, when there is one.
    pub image: Option<ImageSource>,
    /// A video's length.
    pub length: Option<SharedString>,
}

impl std::fmt::Debug for Tile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tile")
            .field("length", &self.length)
            .finish_non_exhaustive()
    }
}

/// How the chat is set up on this account.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChatSettings {
    /// Notifications silenced.
    pub muted: bool,
    /// How long new messages last, when they disappear.
    pub disappearing: Option<Disappearing>,
}

/// The user switched notifications off (`true`) or on; the panel already
/// shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MuteToggled(pub bool);

/// The user chose how long new messages last, or that they stay; the panel
/// already shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisappearingChosen(pub Option<Disappearing>);

impl ProfilePanel {
    /// Media, starred messages, notifications, disappearing messages and
    /// encryption, as on the phone; empty until the app says what it knows.
    pub(crate) fn sections(&self, cx: &mut Context<Self>) -> Div {
        let mut sections = div().w_full().flex().flex_col();
        if let Some(summary) = &self.summary {
            sections = sections
                .child(div().w_full().py_2().child(separator()))
                .child(row(Icon::Camera, "Media, links and docs").child(count(summary.media)))
                .when(!summary.tiles.is_empty(), |sections| {
                    sections.child(
                        div()
                            .flex()
                            .gap_2()
                            .pb_3()
                            .children(summary.tiles.iter().map(tile)),
                    )
                })
                .child(div().w_full().py_2().child(separator()))
                .child(row(Icon::Star, "Starred messages").child(count(summary.starred)));
        }
        if let Some(settings) = self.settings {
            sections = sections
                .child(row(Icon::Muted, "Mute notifications").child(
                    Switch::new("mute-chat", settings.muted).on_toggle({
                        let panel = cx.weak_entity();
                        move |muted, _, cx| {
                            let _ = panel.update(cx, |panel, cx| {
                                if let Some(settings) = &mut panel.settings {
                                    settings.muted = muted;
                                }
                                cx.emit(MuteToggled(muted));
                                cx.notify();
                            });
                        }
                    }),
                ))
                .child(
                    row(Icon::Clock, "Disappearing messages")
                        .id("disappearing")
                        .cursor_pointer()
                        .hover(|row| row.bg(rgb(RAISED)))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|panel, event: &gpui::MouseDownEvent, _, cx| {
                                panel.timer_menu = Some(event.position);
                                cx.notify();
                            }),
                        )
                        .child(
                            div()
                                .text_size(px(13.0))
                                .text_color(rgb(MUTED))
                                .child(settings.disappearing.map_or("Off", Disappearing::label)),
                        ),
                );
        }
        sections
            .child(
                row(Icon::Lock, "Encryption").child(
                    div()
                        .text_size(px(12.5))
                        .text_color(rgb(MUTED))
                        .child("End-to-end encrypted"),
                ),
            )
            .children(self.timer_menu.map(|position| timer_menu(position, cx)))
    }
}

/// The choices for disappearing messages, where the row was clicked.
fn timer_menu(position: Point<Pixels>, cx: &mut Context<ProfilePanel>) -> impl IntoElement {
    let choose = |timer: Option<Disappearing>| {
        let panel = cx.weak_entity();
        move |_: &mut gpui::Window, cx: &mut gpui::App| {
            let _ = panel.update(cx, |panel, cx| {
                panel.timer_menu = None;
                if let Some(settings) = &mut panel.settings {
                    settings.disappearing = timer;
                }
                cx.emit(DisappearingChosen(timer));
                cx.notify();
            });
        }
    };
    let dismiss = {
        let panel = cx.weak_entity();
        move |_: &mut gpui::Window, cx: &mut gpui::App| {
            let _ = panel.update(cx, |panel, cx| {
                panel.timer_menu = None;
                cx.notify();
            });
        }
    };
    let mut menu = Menu::new()
        .on_dismiss(dismiss)
        .action(Icon::Close, "Off", choose(None));
    for timer in Disappearing::ALL {
        menu = menu.action(Icon::Clock, timer.label(), choose(Some(timer)));
    }
    menu.at(position)
}

fn row(icon: Icon, label: &'static str) -> Div {
    div()
        .w_full()
        .h(px(52.0))
        .px_2()
        .flex()
        .items_center()
        .gap_4()
        .rounded_lg()
        .child(
            svg()
                .path(icon.path())
                .size(px(18.0))
                .flex_none()
                .text_color(rgb(MUTED)),
        )
        .child(
            div()
                .flex_1()
                .text_size(px(14.5))
                .text_color(rgb(TEXT))
                .child(label),
        )
}

fn count(count: usize) -> Div {
    div()
        .text_size(px(13.0))
        .text_color(rgb(MUTED))
        .child(count.to_string())
}

fn tile(tile: &Tile) -> Div {
    div()
        .relative()
        .size(px(72.0))
        .flex_none()
        .rounded_lg()
        .bg(rgb(DEEP))
        .children(tile.image.clone().map(|image| {
            img(image)
                .size_full()
                .rounded_lg()
                .object_fit(ObjectFit::Cover)
        }))
        .children(tile.length.clone().map(|length| {
            div()
                .absolute()
                .bottom_1()
                .left_1()
                .px_1()
                .rounded_md()
                .bg(rgba(SCRIM))
                .text_size(px(11.0))
                .text_color(rgb(TEXT))
                .child(length)
        }))
}
