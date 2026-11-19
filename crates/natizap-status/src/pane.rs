//! The Status place: your own status first, then the people whose status
//! holds something unseen, then those whose status was all seen.

use chrono::{Local, TimeZone as _};
use gpui::prelude::*;
use gpui::{
    Context, Div, Entity, EventEmitter, FontWeight, SharedString, Window, div, px, rgb, rgba,
};
use natizap_profiles::ProfilePictures;
use natizap_protocol::ChatId;
use natizap_ui::theme::{ActiveTheme as _, DISPLAY_FONT, Theme};
use natizap_ui::{Button, ButtonVariant, Icon, avatar, status_ring};

use crate::book::StatusBook;
use crate::post::Poster;

/// Side of an avatar in the list.
const AVATAR: f32 = 44.0;

/// The user opened someone's status, or their own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusOpened(pub ChatId);

/// The user wants to post a status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostRequested;

/// The list of status updates, in place of the chat list.
pub struct StatusPane {
    book: Entity<StatusBook>,
    pictures: Entity<ProfilePictures>,
    /// This account's address and name, for its own row.
    me: Option<(ChatId, SharedString)>,
}

impl std::fmt::Debug for StatusPane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatusPane").finish_non_exhaustive()
    }
}

impl EventEmitter<StatusOpened> for StatusPane {}
impl EventEmitter<PostRequested> for StatusPane {}

impl StatusPane {
    /// Lists `book`'s status updates; `me` is this account's address and
    /// name, once linked.
    pub fn new(
        book: Entity<StatusBook>,
        pictures: Entity<ProfilePictures>,
        me: Option<(ChatId, SharedString)>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&book, |_, _, cx| cx.notify()).detach();
        cx.observe(&pictures, |_, _, cx| cx.notify()).detach();
        Self { book, pictures, me }
    }

    fn row(
        &self,
        poster: &Poster,
        title: SharedString,
        unseen: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = *cx.theme();
        let picture = self
            .pictures
            .update(cx, |pictures, cx| pictures.picture(&poster.poster, cx));
        let chat = poster.poster.clone();
        let count = poster.posts.len();
        let detail = if count > 1 {
            format!("{} · {count} updates", when(poster.latest()))
        } else {
            when(poster.latest())
        };
        div()
            .id(SharedString::from(format!(
                "status-{}",
                poster.poster.as_str()
            )))
            .mx_2()
            .px_3()
            .py_2()
            .flex()
            .items_center()
            .gap_3()
            .rounded_lg()
            .cursor_pointer()
            .hover(|row| row.bg(rgba(theme.hover)))
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(StatusOpened(chat.clone()))))
            .child(status_ring(
                avatar(poster.poster.as_str(), &title, picture, px(AVATAR)),
                Some(unseen),
                theme,
            ))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(rgb(theme.muted))
                            .child(detail),
                    ),
            )
    }

    fn mine(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let theme = *cx.theme();
        let (me, name) = self.me.clone()?;
        let mine = self.book.read(cx).mine();
        let picture = self
            .pictures
            .update(cx, |pictures, cx| pictures.picture(&me, cx));
        let detail = mine.as_ref().map_or_else(
            || "Tap to add a status update".to_owned(),
            |mine| when(mine.latest()),
        );
        let posted = mine.is_some();
        Some(
            div()
                .id("status-mine")
                .mx_2()
                .px_3()
                .py_2()
                .flex()
                .items_center()
                .gap_3()
                .rounded_lg()
                .cursor_pointer()
                .hover(|row| row.bg(rgba(theme.hover)))
                .on_click(cx.listener(move |_, _, _, cx| {
                    if posted {
                        cx.emit(StatusOpened(me.clone()));
                    } else {
                        cx.emit(PostRequested);
                    }
                }))
                .child(status_ring(
                    avatar(
                        self.me.as_ref().map_or("", |(me, _)| me.as_str()),
                        &name,
                        picture,
                        px(AVATAR),
                    ),
                    posted.then_some(false),
                    theme,
                ))
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().font_weight(FontWeight::MEDIUM).child("My status"))
                        .child(
                            div()
                                .text_size(px(12.5))
                                .text_color(rgb(theme.muted))
                                .child(detail),
                        ),
                ),
        )
    }
}

impl Render for StatusPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let others = self.book.read(cx).others();
        let (unseen, seen): (Vec<Poster>, Vec<Poster>) =
            others.into_iter().partition(Poster::unseen);
        let rows = |posters: &[Poster], unseen: bool, pane: &Self, cx: &mut Context<Self>| {
            posters
                .iter()
                .map(|poster| {
                    let title = pane.book.read(cx).title(&poster.poster);
                    pane.row(poster, title, unseen, cx).into_any_element()
                })
                .collect::<Vec<_>>()
        };
        let recent = rows(&unseen, true, self, cx);
        let viewed = rows(&seen, false, self, cx);
        let empty = recent.is_empty() && viewed.is_empty();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme.panel))
            .text_color(rgb(theme.text))
            .child(
                div()
                    .flex()
                    .items_center()
                    .px_5()
                    .pt_4()
                    .pb_2()
                    .child(
                        div()
                            .flex_1()
                            .font_family(DISPLAY_FONT)
                            .text_size(px(24.0))
                            .font_weight(FontWeight::BOLD)
                            .child("Status"),
                    )
                    .child(
                        Button::new("post-status")
                            .icon(Icon::Plus)
                            .variant(ButtonVariant::Ghost)
                            .round()
                            .tooltip("Add status")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(PostRequested))),
                    ),
            )
            .child(
                div()
                    .id("status-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .pb_3()
                    .children(self.mine(cx))
                    .when(!recent.is_empty(), |list| {
                        list.child(heading("Recent updates", theme))
                            .children(recent)
                    })
                    .when(!viewed.is_empty(), |list| {
                        list.child(heading("Viewed updates", theme))
                            .children(viewed)
                    })
                    .when(empty, |list| {
                        list.child(
                            div()
                                .px_5()
                                .pt_6()
                                .text_size(px(13.5))
                                .text_color(rgb(theme.muted))
                                .child("No status updates from your contacts in the last day."),
                        )
                    }),
            )
    }
}

fn heading(label: &'static str, theme: Theme) -> Div {
    div()
        .px_5()
        .pt_4()
        .pb_1()
        .text_size(px(13.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(theme.muted))
        .child(label)
}

/// When a post went up: the time today, else "Yesterday" and the time.
pub(crate) fn when(sent_at: i64) -> String {
    let Some(sent) = Local.timestamp_opt(sent_at, 0).single() else {
        return String::new();
    };
    let today = Local::now().date_naive();
    if sent.date_naive() == today {
        format!("Today, {}", sent.format("%H:%M"))
    } else {
        format!("Yesterday, {}", sent.format("%H:%M"))
    }
}
