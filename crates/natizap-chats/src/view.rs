use std::hash::{DefaultHasher, Hash as _, Hasher as _};

use chrono::{DateTime, Datelike as _, Local, TimeZone as _};
use gpui::prelude::*;
use gpui::{Context, Div, Entity, FontWeight, Stateful, Window, div, px, rgb, uniform_list};

use crate::list::{ChatList, ChatRow};

const ROW_HEIGHT: f32 = 72.0;
const AVATAR_SIZE: f32 = 48.0;

const PANEL: u32 = 0x11_1b_21;
const HOVER: u32 = 0x20_2c_33;
const DIVIDER: u32 = 0x22_2d_34;
const TEXT: u32 = 0xe9_ed_ef;
const SECONDARY: u32 = 0x86_96_a0;
const ACCENT: u32 = 0x00_a8_84;

/// Avatar backgrounds; each chat keeps the same one.
const AVATAR_COLORS: [u32; 6] = [
    0x6b_4f_bb, 0x2a_7a_b0, 0x1f_8a_70, 0xb5_6a_2a, 0xa8_3c_5e, 0x5a_6b_7a,
];

/// The chat list column: header, archive entry and the scrollable chats.
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
            div()
                .flex_1()
                .p_6()
                .text_color(rgb(SECONDARY))
                .child("Your chats will appear here once WhatsApp finishes syncing.")
                .into_any_element()
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
                    .px_4()
                    .py_3()
                    .text_xl()
                    .font_weight(FontWeight::BOLD)
                    .child("Chats"),
            )
            .when(archived > 0, |panel| panel.child(archive_row(archived)))
            .child(body)
    }
}

fn archive_row(count: usize) -> Div {
    div()
        .flex()
        .items_center()
        .gap_4()
        .px_4()
        .h(px(48.0))
        .child(div().w(px(AVATAR_SIZE)).flex().justify_center().child("🗄"))
        .child(div().flex_1().child("Archived"))
        .child(
            div()
                .text_sm()
                .text_color(rgb(ACCENT))
                .child(count.to_string()),
        )
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
        div()
            .min_w(px(20.0))
            .h(px(20.0))
            .px_1()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(rgb(if row.muted { SECONDARY } else { ACCENT }))
            .text_color(rgb(PANEL))
            .text_xs()
            .font_weight(FontWeight::BOLD)
            .child(label)
    });

    div()
        .id(gpui::SharedString::from(row.id.as_str().to_owned()))
        .h(px(ROW_HEIGHT))
        .flex()
        .items_center()
        .gap_3()
        .px_3()
        .cursor_pointer()
        .hover(|style| style.bg(rgb(HOVER)))
        .child(avatar(row))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .flex_col()
                .justify_center()
                .gap_1()
                .border_b_1()
                .border_color(rgb(DIVIDER))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(div().flex_1().min_w_0().truncate().child(row.title.clone()))
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(if has_unread { ACCENT } else { SECONDARY }))
                                .child(time),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_sm()
                        .text_color(rgb(SECONDARY))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .children(row.preview.clone()),
                        )
                        .when(row.muted, |line| line.child("🔇"))
                        .when(row.pinned, |line| line.child("📌"))
                        .children(badge),
                ),
        )
}

fn avatar(row: &ChatRow) -> Div {
    let mut hasher = DefaultHasher::new();
    row.id.hash(&mut hasher);
    let color = AVATAR_COLORS
        [usize::try_from(hasher.finish() % AVATAR_COLORS.len() as u64).unwrap_or_default()];
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
        .bg(rgb(color))
        .text_lg()
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
