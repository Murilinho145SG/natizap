use std::hash::{DefaultHasher, Hash as _, Hasher as _};

use chrono::{DateTime, Datelike as _, Local, TimeZone as _};
use gpui::prelude::*;
use gpui::{
    Context, Div, Entity, FontWeight, Stateful, Window, div, px, relative, rgb, svg, uniform_list,
};
use natizap_ui::Icon;
use natizap_ui::theme::{
    AVATAR_TINTS, DISPLAY_FONT, HONEY, HONEY_INK, LINE, MUTED, PANEL, RAISED, TEXT,
};

use crate::list::{ChatList, ChatRow};

const ROW_HEIGHT: f32 = 68.0;
const AVATAR_SIZE: f32 = 44.0;

/// The chat list column: heading, archive entry and the scrollable chats.
#[derive(Debug)]
pub struct ChatListView {
    list: Entity<ChatList>,
}

impl ChatListView {
    /// Creates a view that re-renders whenever `list` changes.
    pub fn new(list: Entity<ChatList>, cx: &mut Context<Self>) -> Self {
        cx.observe(&list, |_, _, cx| cx.notify()).detach();
        Self { list }
    }
}

impl Render for ChatListView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let list = self.list.read(cx);
        let count = list.rows().len();
        let archived = list.archived_count();

        let body = if count == 0 {
            empty_state().into_any_element()
        } else {
            uniform_list(
                "chats",
                count,
                cx.processor(|this, range: std::ops::Range<usize>, _window, cx| {
                    let rows = this.list.read(cx).rows().clone();
                    let now = Local::now();
                    rows[range]
                        .iter()
                        .map(|row| chat_row(row, now))
                        .collect::<Vec<_>>()
                }),
            )
            .flex_1()
            .pb_2()
            .into_any_element()
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(PANEL))
            .text_color(rgb(TEXT))
            .child(
                div()
                    .px_5()
                    .pt_5()
                    .pb_3()
                    .font_family(DISPLAY_FONT)
                    .text_size(px(24.0))
                    .font_weight(FontWeight::BOLD)
                    .child("Chats"),
            )
            .when(archived > 0, |panel| panel.child(archive_row(archived)))
            .child(body)
    }
}

fn empty_state() -> Div {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .px_8()
        .pb_16()
        .child(
            svg()
                .path(Icon::Chats.path())
                .size(px(40.0))
                .text_color(rgb(LINE)),
        )
        .child(
            div()
                .pt_2()
                .font_weight(FontWeight::MEDIUM)
                .child("No chats yet"),
        )
        .child(
            div()
                .text_size(px(13.5))
                .text_center()
                .text_color(rgb(MUTED))
                .child("Messages you send or receive on your phone will show up here."),
        )
}

fn archive_row(count: usize) -> Div {
    div()
        .mx_2()
        .px_3()
        .h(px(44.0))
        .flex()
        .items_center()
        .gap_3()
        .rounded_xl()
        .text_color(rgb(MUTED))
        .child(
            div().w(px(AVATAR_SIZE)).flex().justify_center().child(
                svg()
                    .path(Icon::Archive.path())
                    .size(px(18.0))
                    .text_color(rgb(MUTED)),
            ),
        )
        .child(div().flex_1().text_color(rgb(TEXT)).child("Archived"))
        .child(div().text_size(px(12.5)).child(count.to_string()))
}

fn chat_row(row: &ChatRow, now: DateTime<Local>) -> Stateful<Div> {
    let has_unread = row.unread > 0 || row.marked_unread;
    let time = row
        .sent_at
        .map_or_default(|sent_at| time_label(sent_at, now));

    let badge = has_unread.then(|| {
        let label = if row.unread > 0 {
            row.unread.to_string()
        } else {
            String::new()
        };
        let (background, ink) = if row.muted {
            (LINE, MUTED)
        } else {
            (HONEY, HONEY_INK)
        };
        div()
            .min_w(px(20.0))
            .h(px(20.0))
            .px(px(6.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(rgb(background))
            .text_color(rgb(ink))
            .text_size(px(11.5))
            .font_weight(FontWeight::BOLD)
            .child(label)
    });

    div()
        .id(gpui::SharedString::from(row.id.as_str().to_owned()))
        .h(px(ROW_HEIGHT))
        .mx_2()
        .px_3()
        .flex()
        .items_center()
        .gap_3()
        .rounded_xl()
        .cursor_pointer()
        .hover(|style| style.bg(rgb(RAISED)))
        .child(avatar(row))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(15.0))
                                .font_weight(if has_unread {
                                    FontWeight::SEMIBOLD
                                } else {
                                    FontWeight::MEDIUM
                                })
                                .child(row.title.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(12.0))
                                .text_color(rgb(if has_unread && !row.muted {
                                    HONEY
                                } else {
                                    MUTED
                                }))
                                .child(time),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .text_size(px(13.5))
                        .line_height(relative(1.3))
                        .text_color(rgb(MUTED))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .children(row.preview.clone()),
                        )
                        .when(row.muted, |line| line.child(status_icon(Icon::Muted)))
                        .when(row.pinned, |line| line.child(status_icon(Icon::Pin)))
                        .children(badge),
                ),
        )
}

fn status_icon(icon: Icon) -> impl IntoElement {
    svg()
        .path(icon.path())
        .size(px(15.0))
        .flex_none()
        .text_color(rgb(MUTED))
}

fn avatar(row: &ChatRow) -> Div {
    let mut hasher = DefaultHasher::new();
    row.id.hash(&mut hasher);
    let tint = AVATAR_TINTS
        [usize::try_from(hasher.finish() % AVATAR_TINTS.len() as u64).unwrap_or_default()];
    let initial = row
        .title
        .chars()
        .find(|c| c.is_alphanumeric())
        .map_or_default(|c| c.to_uppercase().collect::<String>());
    div()
        .size(px(AVATAR_SIZE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(rgb(tint))
        .font_family(DISPLAY_FONT)
        .text_size(px(18.0))
        .font_weight(FontWeight::SEMIBOLD)
        .child(initial)
}

/// When a message was sent, as the chat list shows it: the time today,
/// "Yesterday", the weekday within the last week, and the date before that.
fn time_label(sent_at: i64, now: DateTime<Local>) -> String {
    let Some(sent) = Local.timestamp_opt(sent_at, 0).single() else {
        return String::new();
    };
    let days_ago = (now.date_naive() - sent.date_naive()).num_days();
    match days_ago {
        ..=0 => sent.format("%H:%M").to_string(),
        1 => "Yesterday".to_owned(),
        2..=6 => sent.format("%A").to_string(),
        _ if sent.year() == now.year() => sent.format("%d/%m").to_string(),
        _ => sent.format("%d/%m/%Y").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;

    fn at(now: DateTime<Local>, ago: Duration) -> i64 {
        (now - ago).timestamp()
    }

    #[test]
    fn labels_recent_messages_like_the_mobile_app() {
        let now = Local.with_ymd_and_hms(2026, 11, 16, 15, 30, 0).single();
        let Some(now) = now else {
            panic!("test date must exist in the local zone")
        };

        assert_eq!(time_label(at(now, Duration::minutes(5)), now), "15:25");
        assert_eq!(time_label(at(now, Duration::days(1)), now), "Yesterday");
        assert_eq!(time_label(at(now, Duration::days(3)), now), "Friday");
        assert_eq!(time_label(at(now, Duration::days(30)), now), "17/10");
        assert_eq!(time_label(at(now, Duration::days(400)), now), "12/10/2025");
    }
}
