use chrono::{DateTime, Datelike as _, Local, TimeZone as _};
use gpui::prelude::*;
use gpui::{
    AnyElement, AnyView, Context, Div, Entity, EventEmitter, FontWeight, MouseButton,
    MouseDownEvent, SharedString, Stateful, UniformListScrollHandle, Window, div, px, relative,
    rgb, svg, uniform_list,
};
use natizap_ui::theme::{ActiveTheme as _, DISPLAY_FONT, Theme};
use natizap_ui::{
    BadgeTone, Button, ButtonVariant, Icon, TextInput, avatar, badge, count_label, scrollbar,
    separator, skeleton, status_ring, verified_seal,
};

use std::collections::HashMap;

use natizap_profiles::ProfilePictures;
use natizap_protocol::{ChatId, ContentKind};

use crate::filters::{Filter, chips, narrowed, search_field};
use crate::list::{ChatList, ChatRow};

mod menu;
mod searching;

pub use menu::ChatForgotten;
use menu::ChatPopup;
pub use searching::QueryChanged;

const ROW_HEIGHT: f32 = 68.0;
const AVATAR_SIZE: f32 = 44.0;

/// The chat list column: heading, archive entry and the scrollable chats.
#[derive(Debug)]
pub struct ChatListView {
    list: Entity<ChatList>,
    pictures: Entity<ProfilePictures>,
    selected: Option<ChatId>,
    syncing: bool,
    search: Entity<TextInput>,
    filter: Filter,
    /// The archived chats are shown instead of the rest.
    in_archive: bool,
    /// How far the list is scrolled, for its scrollbar.
    scroll: UniformListScrollHandle,
    /// The chats matching a search, above the messages holding it.
    search_scroll: UniformListScrollHandle,
    /// What is typed in the search field, as last told.
    query: String,
    /// The messages holding what is typed, as the app hands them over.
    message_results: Option<AnyView>,
    /// Who has a status, and whether it holds something unseen.
    rings: HashMap<ChatId, bool>,
    /// A chat's right-click menu or one of its questions, when open.
    popup: Option<ChatPopup>,
}

/// The user picked a chat in the list.
#[derive(Debug, Clone)]
pub struct ChatSelected(pub ChatRow);

impl EventEmitter<ChatSelected> for ChatListView {}
impl EventEmitter<QueryChanged> for ChatListView {}
impl EventEmitter<ChatForgotten> for ChatListView {}

/// The user clicked the ringed avatar of someone with a status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusRequested(pub ChatId);

impl EventEmitter<StatusRequested> for ChatListView {}

impl ChatListView {
    /// Creates a view that re-renders whenever `list` changes or a chat's
    /// picture arrives.
    pub fn new(
        list: Entity<ChatList>,
        pictures: Entity<ProfilePictures>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&list, |_, _, cx| cx.notify()).detach();
        cx.observe(&pictures, |_, _, cx| cx.notify()).detach();
        let search = cx.new(|cx| TextInput::new("Search", cx));
        // Typing narrows the list, and searches the messages.
        cx.observe(&search, |view, _, cx| {
            view.query_changed(cx);
            cx.notify();
        })
        .detach();
        Self {
            list,
            pictures,
            selected: None,
            syncing: false,
            search,
            filter: Filter::All,
            in_archive: false,
            scroll: UniformListScrollHandle::new(),
            search_scroll: UniformListScrollHandle::new(),
            query: String::new(),
            message_results: None,
            rings: HashMap::new(),
            popup: None,
        }
    }

    /// Whether the first sync after linking is still under way: an empty
    /// list then shows placeholders rather than "no chats".
    pub fn set_syncing(&mut self, syncing: bool, cx: &mut Context<Self>) {
        if self.syncing != syncing {
            self.syncing = syncing;
            cx.notify();
        }
    }

    /// Rings the avatars of people with a status, by every address they go
    /// by: `true` while it holds something unseen.
    pub fn set_status_rings(&mut self, rings: HashMap<ChatId, bool>, cx: &mut Context<Self>) {
        self.rings = rings;
        cx.notify();
    }

    /// Marks `chat` as the one on screen, e.g. after going back to it, or
    /// none once the chat closes.
    pub fn show(&mut self, chat: Option<&ChatId>, cx: &mut Context<Self>) {
        self.selected = chat.cloned();
        cx.notify();
    }

    fn select(&mut self, row: ChatRow, cx: &mut Context<Self>) {
        self.selected = Some(row.id.clone());
        cx.emit(ChatSelected(row));
        cx.notify();
    }
}

impl Render for ChatListView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let list = self.list.read(cx);
        let source = if self.in_archive {
            list.archived()
        } else {
            list.rows()
        };
        let query = self.search.read(cx).text().to_owned();
        let rows = narrowed(source, self.filter, &query);
        let unread = list
            .rows()
            .iter()
            .filter(|row| row.unread > 0 || row.marked_unread)
            .count();
        let archived = list.archived().len();
        let narrowing = !query.trim().is_empty() || self.filter != Filter::All;

        let nothing_yet = source.is_empty();
        let body = if let Some(searching) = self.searching(rows.clone(), cx) {
            searching
        } else if rows.is_empty() && nothing_yet && self.syncing {
            placeholder_rows(theme).into_any_element()
        } else if rows.is_empty() && !narrowing && !self.in_archive {
            empty_state(theme).into_any_element()
        } else {
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(Self::chats(rows, &self.scroll, cx))
                .child(scrollbar("chats-scrollbar", &self.scroll, window, cx))
                .into_any_element()
        };

        let view = cx.weak_entity();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme.panel))
            .text_color(rgb(theme.text))
            .child(if self.in_archive {
                archive_header(cx)
            } else {
                div().flex().items_center().px_5().pt_4().pb_2().child(
                    div()
                        .flex_1()
                        .font_family(DISPLAY_FONT)
                        .text_size(px(24.0))
                        .font_weight(FontWeight::BOLD)
                        .child("Chats"),
                )
            })
            .child(search_field(&self.search, theme))
            .when(!self.in_archive, |panel| {
                panel.child(div().pt_2p5().child(chips(
                    self.filter,
                    unread,
                    move |filter, cx| {
                        let _ = view.update(cx, |view, cx| {
                            view.filter = filter;
                            cx.notify();
                        });
                    },
                    theme,
                )))
            })
            .child(div().h_2())
            .when(!self.in_archive && !narrowing && archived > 0, |panel| {
                panel
                    .child(
                        archive_row(archived, theme).on_click(cx.listener(|view, _, _, cx| {
                            view.in_archive = true;
                            cx.notify();
                        })),
                    )
                    .child(div().mx_5().my_1().child(separator(theme)))
            })
            .child(body)
            .children(self.popup(cx))
    }
}

impl ChatListView {
    /// `rows` as a lazily drawn list: only rows on screen are built.
    fn chats(
        rows: Vec<ChatRow>,
        scroll: &UniformListScrollHandle,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = *cx.theme();
        let count = rows.len();
        uniform_list(
            "chats",
            count,
            cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                let now = Local::now();
                rows[range]
                    .iter()
                    .map(|row| {
                        let selected = this.selected.as_ref() == Some(&row.id)
                            || matches!(
                                &this.popup,
                                Some(ChatPopup::Menu(chat, _)) if *chat == row.id
                            );
                        let clicked = row.clone();
                        let pressed = row.id.clone();
                        // Only rows on screen ask, so pictures load as the
                        // list scrolls.
                        let picture = this
                            .pictures
                            .update(cx, |pictures, cx| pictures.picture(&row.id, cx));
                        let face = avatar(row.id.as_str(), &row.title, picture, px(AVATAR_SIZE));
                        let ring = row
                            .addresses
                            .iter()
                            .find_map(|address| this.rings.get(address).copied());
                        let face = match ring {
                            Some(unseen) => {
                                let poster = row.id.clone();
                                status_ring(face, Some(unseen), theme)
                                    .id(SharedString::from(format!("ring-{}", row.id.as_str())))
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        // The row behind would open the chat.
                                        cx.stop_propagation();
                                        cx.emit(StatusRequested(poster.clone()));
                                    }))
                                    .into_any_element()
                            }
                            None => status_ring(face, None, theme).into_any_element(),
                        };
                        let typing = this.list.read(cx).typing_line(row);
                        div().w_full().px_2().child(
                            chat_row(row, face, typing, now, theme)
                                .when(selected, |row| row.bg(rgb(theme.raised)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.select(clicked.clone(), cx);
                                }))
                                .on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                        this.open_menu(pressed.clone(), event.position, cx);
                                    }),
                                ),
                        )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(scroll)
        .flex_1()
        .pb_2()
    }
}

/// The archived chats' heading, with the way back to the rest.
fn archive_header(cx: &mut Context<ChatListView>) -> Div {
    let theme = *cx.theme();
    div()
        .flex()
        .flex_col()
        .gap_2()
        .px_3()
        .pt_4()
        .pb_3()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    Button::new("leave-archive")
                        .icon(Icon::Back)
                        .variant(ButtonVariant::Ghost)
                        .round()
                        .tooltip("Back")
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.in_archive = false;
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .text_size(px(17.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Archived"),
                ),
        )
        .child(
            div()
                .px_2()
                .text_size(px(13.0))
                .text_color(rgb(theme.muted))
                .child(
                    "These chats stay archived when new messages arrive. To change this, \
                     open WhatsApp on your phone: Settings > Chats.",
                ),
        )
}

/// Chat-shaped placeholders while the first sync fills the list.
fn placeholder_rows(theme: Theme) -> Div {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .px_2()
        .children((0..7_usize).map(|index| {
            div()
                .h(px(ROW_HEIGHT))
                .px_3()
                .flex()
                .items_center()
                .gap_3()
                .child(skeleton(
                    ("chat-avatar", index),
                    div().size(px(AVATAR_SIZE)).flex_none().rounded_full(),
                    theme,
                ))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(skeleton(
                            ("chat-title", index),
                            div().w(relative(0.45)).h(px(12.0)).rounded_md(),
                            theme,
                        ))
                        .child(skeleton(
                            ("chat-preview", index),
                            div().w(relative(0.8)).h(px(10.0)).rounded_md(),
                            theme,
                        )),
                )
        }))
}

fn empty_state(theme: Theme) -> Div {
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
                .text_color(rgb(theme.line)),
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
                .text_color(rgb(theme.muted))
                .child("Messages you send or receive on your phone will show up here."),
        )
}

fn archive_row(count: usize, theme: Theme) -> Stateful<Div> {
    div()
        .id("archived")
        .mx_2()
        .px_3()
        .h(px(44.0))
        .flex()
        .items_center()
        .gap_3()
        .rounded_xl()
        .cursor_pointer()
        .hover(|row| row.bg(rgb(theme.raised)))
        .text_color(rgb(theme.muted))
        .child(
            div().w(px(AVATAR_SIZE)).flex().justify_center().child(
                svg()
                    .path(Icon::Archive.path())
                    .size(px(18.0))
                    .text_color(rgb(theme.muted)),
            ),
        )
        .child(div().flex_1().text_color(rgb(theme.text)).child("Archived"))
        .child(div().text_size(px(12.5)).child(count.to_string()))
}

/// One chat's row; `typing` says who types in it, in place of the last
/// message.
fn chat_row(
    row: &ChatRow,
    face: AnyElement,
    typing: Option<SharedString>,
    now: DateTime<Local>,
    theme: Theme,
) -> Stateful<Div> {
    let has_unread = row.unread > 0 || row.marked_unread;
    let typed = typing.is_some();
    let time = row
        .sent_at
        .map_or_default(|sent_at| time_label(sent_at, now));

    let badge = has_unread.then(|| unread_badge(row, theme));

    div()
        .id(SharedString::from(row.id.as_str().to_owned()))
        .h(px(ROW_HEIGHT))
        .w_full()
        .px_3()
        .flex()
        .items_center()
        .gap_3()
        .rounded_xl()
        .cursor_pointer()
        .hover(|style| style.bg(rgb(theme.raised)))
        .active(|style| style.bg(rgb(theme.line)))
        .child(face)
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
                                .flex()
                                .items_center()
                                .gap_1()
                                .text_size(px(15.0))
                                .font_weight(if has_unread {
                                    FontWeight::SEMIBOLD
                                } else {
                                    FontWeight::MEDIUM
                                })
                                .child(div().min_w_0().truncate().child(row.title.clone()))
                                .when(row.verified, |title| {
                                    title.child(verified_seal(15.0, theme))
                                }),
                        )
                        .child(
                            div()
                                .text_size(px(12.0))
                                .text_color(rgb(if has_unread && !row.muted {
                                    theme.accent
                                } else {
                                    theme.muted
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
                        .text_color(rgb(theme.muted))
                        .when(!typed, |line| {
                            line.children(
                                row.preview_kind
                                    .and_then(kind_icon)
                                    .map(|icon| status_icon(icon, theme)),
                            )
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .map(|line| match typing {
                                    Some(typing) => {
                                        line.text_color(rgb(theme.accent)).child(typing)
                                    }
                                    None => line.children(row.preview.clone()),
                                }),
                        )
                        .when(row.muted, |line| {
                            line.child(status_icon(Icon::Muted, theme))
                        })
                        .when(row.pinned, |line| line.child(status_icon(Icon::Pin, theme)))
                        .children(badge),
                ),
        )
}

/// How many messages are unread, or a dot for a chat marked unread; grey
/// when the chat is muted.
fn unread_badge(row: &ChatRow, theme: Theme) -> impl IntoElement + use<> {
    let label = if row.unread > 0 {
        count_label(row.unread)
    } else {
        String::new()
    };
    let tone = if row.muted {
        BadgeTone::Muted
    } else {
        BadgeTone::Accent
    };
    badge(label, tone, theme)
}

fn status_icon(icon: Icon, theme: Theme) -> impl IntoElement {
    svg()
        .path(icon.path())
        .size(px(15.0))
        .flex_none()
        .text_color(rgb(theme.muted))
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

/// The icon a preview of `kind` shows before its text, as the phone does.
fn kind_icon(kind: ContentKind) -> Option<Icon> {
    Some(match kind {
        ContentKind::Text => return None,
        ContentKind::Photo => Icon::Camera,
        ContentKind::Video | ContentKind::VideoCall => Icon::Video,
        ContentKind::VoiceMessage => Icon::Mic,
        ContentKind::Audio => Icon::Waveform,
        ContentKind::Document => Icon::File,
        ContentKind::Sticker => Icon::Sticker,
        ContentKind::Location => Icon::Location,
        ContentKind::Contact => Icon::Contact,
        ContentKind::Poll => Icon::Poll,
        ContentKind::MissedVoiceCall | ContentKind::MissedVideoCall => Icon::CallMissed,
        ContentKind::VoiceCall => Icon::Call,
    })
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
