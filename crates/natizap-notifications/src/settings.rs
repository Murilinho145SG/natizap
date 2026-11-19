//! The notification settings page: whether chats and groups chime, with
//! which tone, and whether other chats chime while natizap is in use.

use gpui::prelude::*;
use gpui::{
    App, Context, Div, Entity, FontWeight, SharedString, Stateful, Window, div, px, rgb, svg,
};
use natizap_ui::theme::ActiveTheme as _;
use natizap_ui::{Icon, Switch, separator};

use crate::{Alert, Notifier, Preferences, Tone};

/// Changes a [`Notifier`]'s preferences: the notifications page of the
/// settings window.
#[derive(Debug)]
pub struct NotificationSettings {
    notifier: Entity<Notifier>,
}

impl NotificationSettings {
    /// Shows and changes `notifier`'s preferences.
    pub fn new(notifier: Entity<Notifier>, cx: &mut Context<Self>) -> Self {
        cx.observe(&notifier, |_, _, cx| cx.notify()).detach();
        Self { notifier }
    }

    /// Applies `change` to the preferences.
    fn change(&self, change: impl FnOnce(&mut Preferences), cx: &mut App) {
        self.notifier.update(cx, |notifier, cx| {
            let mut preferences = notifier.preferences();
            change(&mut preferences);
            notifier.set_preferences(preferences, cx);
        });
    }

    /// One kind of message's sound: whether it plays, and its tone.
    fn alert(kind: Kind, alert: Alert, cx: &mut Context<Self>) -> Div {
        let theme = *cx.theme();
        let this = cx.weak_entity();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .px_5()
            .py_3()
            .child(
                div()
                    .text_size(px(13.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(theme.muted))
                    .child(kind.heading()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(div().flex_1().child("Play a sound"))
                    .child(Switch::new(kind.id("sound"), alert.sound).on_toggle(
                        move |sound, _, cx| {
                            let _ = this.update(cx, |this, cx| {
                                this.change(
                                    |preferences| kind.alert(preferences).sound = sound,
                                    cx,
                                );
                            });
                        },
                    )),
            )
            .when(alert.sound, |section| {
                section.child(
                    div().flex().gap_2().children(
                        Tone::ALL.map(|tone| Self::tone_chip(kind, tone, alert.tone, cx)),
                    ),
                )
            })
    }

    /// A tone to choose; choosing it plays it.
    fn tone_chip(kind: Kind, tone: Tone, chosen: Tone, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = *cx.theme();
        let selected = tone == chosen;
        let tint = if selected { theme.accent } else { theme.text };
        div()
            .id(SharedString::from(format!(
                "{}-{}",
                kind.id("tone"),
                tone.name()
            )))
            .flex()
            .items_center()
            .gap_1p5()
            .px_3()
            .py_1p5()
            .rounded_full()
            .border_1()
            .border_color(rgb(if selected { theme.accent } else { theme.line }))
            .text_size(px(13.0))
            .text_color(rgb(tint))
            .cursor_pointer()
            .hover(|chip| chip.bg(rgb(theme.raised)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.change(|preferences| kind.alert(preferences).tone = tone, cx);
                this.notifier.read(cx).preview(tone);
            }))
            .child(
                svg()
                    .path(Icon::Play.path())
                    .size(px(11.0))
                    .text_color(rgb(tint)),
            )
            .child(tone.name())
    }
}

impl NotificationSettings {
    /// A setting that is on or off, with what it does below its name.
    fn switch_row(setting: Setting, on: bool, cx: &mut Context<Self>) -> Div {
        let theme = *cx.theme();
        let this = cx.weak_entity();
        let (label, description) = setting.text();
        div()
            .flex()
            .items_center()
            .gap_3()
            .px_5()
            .py_3()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(label)
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(rgb(theme.muted))
                            .child(description),
                    ),
            )
            .child(
                Switch::new(setting.text().0, on).on_toggle(move |on, _, cx| {
                    let _ = this.update(cx, |this, cx| {
                        this.change(|preferences| *setting.value(preferences) = on, cx);
                    });
                }),
            )
    }
}

/// The settings that are only on or off.
#[derive(Debug, Clone, Copy)]
enum Setting {
    OtherChats,
    Popups,
    Previews,
}

impl Setting {
    /// Its name and what it does.
    fn text(self) -> (&'static str, &'static str) {
        match self {
            Self::OtherChats => (
                "Sounds for other chats",
                "While natizap is in use, chime for chats other than the one open.",
            ),
            Self::Popups => (
                "System notifications",
                "While natizap is in the background, show new messages in the \
                 system's notifications.",
            ),
            Self::Previews => (
                "Show message preview",
                "Show what the message says in the notification, not just who sent it.",
            ),
        }
    }

    fn value(self, preferences: &mut Preferences) -> &mut bool {
        match self {
            Self::OtherChats => &mut preferences.other_chats,
            Self::Popups => &mut preferences.popups,
            Self::Previews => &mut preferences.previews,
        }
    }
}

/// The kinds of message with a sound of their own.
#[derive(Debug, Clone, Copy)]
enum Kind {
    Messages,
    Groups,
}

impl Kind {
    fn heading(self) -> &'static str {
        match self {
            Self::Messages => "Messages",
            Self::Groups => "Groups",
        }
    }

    fn id(self, part: &str) -> SharedString {
        format!("{}-{part}", self.heading().to_lowercase()).into()
    }

    fn alert(self, preferences: &mut Preferences) -> &mut Alert {
        match self {
            Self::Messages => &mut preferences.messages,
            Self::Groups => &mut preferences.groups,
        }
    }
}

impl Render for NotificationSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let preferences = self.notifier.read(cx).preferences();
        div()
            .flex()
            .flex_col()
            .child(Self::alert(Kind::Messages, preferences.messages, cx))
            .child(Self::alert(Kind::Groups, preferences.groups, cx))
            .child(div().px_5().py_1().child(separator(theme)))
            .child(Self::switch_row(
                Setting::OtherChats,
                preferences.other_chats,
                cx,
            ))
            .child(div().px_5().py_1().child(separator(theme)))
            .child(Self::switch_row(Setting::Popups, preferences.popups, cx))
            .when(preferences.popups, |panel| {
                panel.child(Self::switch_row(
                    Setting::Previews,
                    preferences.previews,
                    cx,
                ))
            })
    }
}
