//! The Calls place: the call history, newest first, missed calls in the
//! alert colour, back-to-back calls with the same person as one row.

use std::collections::HashSet;

use chrono::{Local, TimeZone as _};
use gpui::prelude::*;
use gpui::{
    Context, Entity, EventEmitter, FontWeight, SharedString, Window, div, px, rgb, rgba, svg,
};
use natizap_profiles::ProfilePictures;
use natizap_protocol::{CallOutcome, CallRecord, ChatId};
use natizap_ui::theme::{ALERT, DISPLAY_FONT, HOVER, MUTED, PANEL, TEXT};
use natizap_ui::{Button, ButtonVariant, Icon, avatar};

use crate::group::CallGroup;
use crate::log::CallLog;

const AVATAR: f32 = 44.0;

/// The user picked a call: the app opens its chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallChatRequested(pub ChatId);

/// The call history, in place of the chat list.
pub struct CallsPane {
    log: Entity<CallLog>,
    pictures: Entity<ProfilePictures>,
    /// Rows opened to show each call, by their newest call's id.
    open: HashSet<String>,
}

impl std::fmt::Debug for CallsPane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallsPane").finish_non_exhaustive()
    }
}

impl EventEmitter<CallChatRequested> for CallsPane {}

impl CallsPane {
    /// Shows `log`'s calls.
    pub fn new(
        log: Entity<CallLog>,
        pictures: Entity<ProfilePictures>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&log, |_, _, cx| cx.notify()).detach();
        cx.observe(&pictures, |_, _, cx| cx.notify()).detach();
        Self {
            log,
            pictures,
            open: HashSet::new(),
        }
    }

    fn row(&self, group: &CallGroup, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let call = group.latest().clone();
        let chat = call.group.clone().unwrap_or_else(|| call.peer.clone());
        let title = self.log.read(cx).title(&chat);
        let picture = self
            .pictures
            .update(cx, |pictures, cx| pictures.picture(&chat, cx));
        let count = group.calls.len();
        let opened = self.open.contains(&call.id);
        let missed = call.outcome == CallOutcome::Missed;
        let name = if count > 1 {
            format!("{title} ({count})")
        } else {
            title.to_string()
        };
        let (key, opening) = (call.id.clone(), chat.clone());
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .id(SharedString::from(format!("call-{}", call.id)))
                    .mx_2()
                    .px_3()
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_3()
                    .rounded_lg()
                    .cursor_pointer()
                    .hover(|row| row.bg(rgba(HOVER)))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(CallChatRequested(opening.clone()));
                    }))
                    .child(avatar(chat.as_str(), &title, picture, px(AVATAR)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .font_weight(FontWeight::MEDIUM)
                                    .when(missed && call.incoming, |name| {
                                        name.text_color(rgb(ALERT))
                                    })
                                    .child(name),
                            )
                            .child(summary(&call)),
                    )
                    .when(count > 1, |row| {
                        row.child(
                            Button::new(SharedString::from(format!("call-more-{}", call.id)))
                                .icon(if opened {
                                    Icon::ChevronUp
                                } else {
                                    Icon::ChevronDown
                                })
                                .variant(ButtonVariant::Ghost)
                                .round()
                                .tooltip(if opened {
                                    "Show less"
                                } else {
                                    "Show each call"
                                })
                                .on_click(cx.listener(move |pane, _, _, cx| {
                                    cx.stop_propagation();
                                    if !pane.open.remove(&key) {
                                        pane.open.insert(key.clone());
                                    }
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .when(opened, |row| {
                row.children(group.calls.iter().map(|call| {
                    div()
                        .pl(px(AVATAR + 40.0))
                        .pr_5()
                        .pb_1()
                        .child(summary(call))
                }))
            })
    }
}

impl Render for CallsPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let groups = self.log.read(cx).groups();
        let rows: Vec<_> = groups
            .iter()
            .map(|group| self.row(group, cx).into_any_element())
            .collect();
        let empty = rows.is_empty();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(PANEL))
            .text_color(rgb(TEXT))
            .child(
                div()
                    .px_5()
                    .pt_4()
                    .pb_2()
                    .font_family(DISPLAY_FONT)
                    .text_size(px(24.0))
                    .font_weight(FontWeight::BOLD)
                    .child("Calls"),
            )
            .child(
                div()
                    .id("call-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .pb_3()
                    .children(rows)
                    .when(empty, |list| {
                        list.child(
                            div()
                                .px_5()
                                .pt_6()
                                .text_size(px(13.5))
                                .text_color(rgb(MUTED))
                                .child("No calls yet."),
                        )
                    }),
            )
    }
}

/// One call's line: which way it went, voice or video, when, and how long.
fn summary(call: &CallRecord) -> impl IntoElement + use<> {
    let missed = call.outcome == CallOutcome::Missed;
    let (icon, word) = match (call.incoming, call.outcome) {
        (true, CallOutcome::Missed) => (Icon::CallMissed, "Missed"),
        (true, CallOutcome::Declined) => (Icon::CallMissed, "Declined"),
        (true, CallOutcome::Answered) => (Icon::Call, "Incoming"),
        (false, CallOutcome::Missed) => (Icon::Call, "Not answered"),
        (false, CallOutcome::Declined) => (Icon::Call, "Declined"),
        (false, CallOutcome::Answered) => (Icon::Call, "Outgoing"),
    };
    let kind = if call.video { "video" } else { "voice" };
    let length = call.seconds.map(|seconds| {
        if seconds >= 60 {
            format!(" · {} min", seconds.div_ceil(60))
        } else {
            format!(" · {seconds} s")
        }
    });
    div()
        .flex()
        .items_center()
        .gap_1p5()
        .text_size(px(12.5))
        .text_color(rgb(MUTED))
        .child(
            svg()
                .path(if call.video {
                    Icon::Video.path()
                } else {
                    icon.path()
                })
                .size(px(14.0))
                .flex_none()
                .text_color(rgb(if missed && call.incoming {
                    ALERT
                } else {
                    MUTED
                })),
        )
        .child(format!(
            "{word} {kind} · {}{}",
            when(call.at),
            length.unwrap_or_default()
        ))
}

/// When a call rang: the time today, else the date and time.
fn when(at: i64) -> String {
    let Some(rang) = Local.timestamp_opt(at, 0).single() else {
        return String::new();
    };
    if rang.date_naive() == Local::now().date_naive() {
        rang.format("%H:%M").to_string()
    } else {
        rang.format("%d/%m %H:%M").to_string()
    }
}
