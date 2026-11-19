//! A status played over the whole window, post after post and poster
//! after poster: bars at the top fill as each post shows, a press holds
//! it, the sides step back and on, up and down change person, and a bar
//! at the bottom replies.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    AnyElement, AnyView, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    KeyDownEvent, SharedString, Window, anchored, deferred, div, point, px, relative, rgb, rgba,
};
use natizap_playback::{Heard, Recorder};
use natizap_profiles::ProfilePictures;
use natizap_protocol::{ChatId, ContentKind, Reply, StatusLink};
use natizap_ui::theme::ActiveTheme as _;
use natizap_ui::{Button, ButtonVariant, Icon, Submitted, TextInput, avatar};

use crate::book::StatusBook;
use crate::pane::when;
use crate::post::{Post, Poster};

mod reply;
mod stage;

/// How long a text or photo shows.
const STILL: Duration = Duration::from_secs(5);
/// How long a video shows when it does not say.
const VIDEO: Duration = Duration::from_secs(15);
/// A press shorter than this is a tap that steps; longer, a hold.
const TAP: Duration = Duration::from_millis(220);
/// Quick reactions, sent as a reply, as on the phone.
const QUICK: [&str; 6] = ["❤️", "😂", "😮", "😢", "🙏", "👏"];

/// The user closed the status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusClosed;

/// What a reply to a status carries.
#[derive(Debug, Clone)]
pub enum ReplyBody {
    Text(String),
    Voice(Heard),
}

/// The user replied to a status: the app sends it to the poster's chat,
/// quoting the status.
#[derive(Debug, Clone)]
pub struct StatusReply {
    /// Whose status; the chat the reply goes to.
    pub poster: ChatId,
    /// The status quoted.
    pub reply: Reply,
    pub body: ReplyBody,
}

/// A video post is on screen; the app answers with a player through
/// [`StatusViewer::show_player`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusPlayRequested {
    /// The post, to hand the player back with.
    pub id: SharedString,
    pub file: PathBuf,
}

/// The status being watched.
pub struct StatusViewer {
    book: Entity<StatusBook>,
    pictures: Entity<ProfilePictures>,
    /// The posters walked through, in order; only one for your own.
    queue: Vec<ChatId>,
    mine: bool,
    at_poster: usize,
    at_post: usize,
    /// When the post on screen started showing, held time left out.
    started: Instant,
    /// Since when it is held, if it is.
    held: Option<Instant>,
    /// Since when the space bar paused it; it stays held until the space
    /// bar is pressed again, whatever else lets go.
    paused: Option<Instant>,
    /// When the pointer went down on the stage.
    pressed: Option<Instant>,
    player: Option<(String, AnyView)>,
    /// The post a player was asked for.
    asked: Option<String>,
    reply: Entity<TextInput>,
    recorder: Option<Recorder>,
    /// Who saw your post shows.
    viewers_open: bool,
    /// The post on screen ran out and the next is about to show.
    advancing: bool,
    focus: FocusHandle,
}

impl std::fmt::Debug for StatusViewer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatusViewer")
            .field("at_poster", &self.at_poster)
            .field("at_post", &self.at_post)
            .finish_non_exhaustive()
    }
}

impl EventEmitter<StatusClosed> for StatusViewer {}
impl EventEmitter<StatusReply> for StatusViewer {}
impl EventEmitter<StatusPlayRequested> for StatusViewer {}

impl Focusable for StatusViewer {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl StatusViewer {
    /// Plays `start`'s status, then the next posters' after it; your own
    /// alone when `start` is you.
    pub fn new(
        book: Entity<StatusBook>,
        pictures: Entity<ProfilePictures>,
        start: &ChatId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (mine, queue) = {
            let book = book.read(cx);
            if let Some(mine) = book.mine().filter(|mine| mine.poster == *start) {
                (true, vec![mine.poster])
            } else {
                let others: Vec<ChatId> = book
                    .others()
                    .into_iter()
                    .map(|poster| poster.poster)
                    .collect();
                let from = others.iter().position(|chat| chat == start).unwrap_or(0);
                (false, others[from..].to_vec())
            }
        };
        let reply = cx.new(|cx| TextInput::new("Reply…", cx));
        cx.subscribe_in(&reply, window, |viewer, _, Submitted(text), window, cx| {
            let text = text.trim().to_owned();
            if !text.is_empty() {
                viewer.send_reply(ReplyBody::Text(text), window, cx);
            }
        })
        .detach();
        // A video downloaded while on screen starts playing.
        cx.observe(&book, |viewer, _, cx| {
            viewer.ask_player(cx);
            cx.notify();
        })
        .detach();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut viewer = Self {
            book,
            pictures,
            queue,
            mine,
            at_poster: 0,
            at_post: 0,
            started: Instant::now(),
            held: None,
            paused: None,
            pressed: None,
            player: None,
            asked: None,
            reply,
            recorder: None,
            viewers_open: false,
            advancing: false,
            focus,
        };
        viewer.at_post = viewer.poster(cx).map_or(0, |poster| poster.start());
        viewer.showing(cx);
        viewer
    }

    /// Plays `player`, the app's answer to a [`StatusPlayRequested`], if
    /// its post is still on screen.
    pub fn show_player(&mut self, id: &str, player: AnyView, cx: &mut Context<Self>) {
        if self.post(cx).is_some_and(|post| post.id == id) {
            self.player = Some((id.to_owned(), player));
            // Its clock starts with the video, not with the download.
            self.started = Instant::now();
            cx.notify();
        }
    }

    fn poster(&self, cx: &gpui::App) -> Option<Poster> {
        let chat = self.queue.get(self.at_poster)?;
        let book = self.book.read(cx);
        if self.mine {
            book.mine()
        } else {
            book.others()
                .into_iter()
                .find(|poster| poster.poster == *chat)
        }
    }

    fn post(&self, cx: &gpui::App) -> Option<Post> {
        self.poster(cx)?.posts.get(self.at_post).cloned()
    }

    /// A new post came on screen: start its clock, mark it seen and ask
    /// for a player if it is a video.
    fn showing(&mut self, cx: &mut Context<Self>) {
        self.started = Instant::now();
        self.held = None;
        self.paused = None;
        self.player = None;
        self.viewers_open = false;
        let Some(post) = self.post(cx) else {
            return;
        };
        self.book.update(cx, |book, cx| {
            book.mark_seen(&post, cx);
            book.want(&post, cx);
        });
        self.ask_player(cx);
        cx.notify();
    }

    /// Asks the app for a player once the video on screen is on disk.
    fn ask_player(&mut self, cx: &mut Context<Self>) {
        let Some(post) = self.post(cx) else {
            return;
        };
        if post.kind == ContentKind::Video
            && let Some(file) = post.file
            && self.asked.as_deref() != Some(post.id.as_str())
        {
            self.asked = Some(post.id.clone());
            cx.emit(StatusPlayRequested {
                id: post.id.into(),
                file,
            });
        }
    }

    fn forward(&mut self, cx: &mut Context<Self>) {
        let count = self.poster(cx).map_or(0, |poster| poster.posts.len());
        if self.at_post + 1 < count {
            self.at_post += 1;
        } else if self.at_poster + 1 < self.queue.len() {
            self.at_poster += 1;
            self.at_post = self.poster(cx).map_or(0, |poster| poster.start());
        } else {
            cx.emit(StatusClosed);
            return;
        }
        self.showing(cx);
    }

    /// Skips to the next person's statuses, where they are unseen.
    fn next_poster(&mut self, cx: &mut Context<Self>) {
        if self.at_poster + 1 < self.queue.len() {
            self.at_poster += 1;
            self.at_post = self.poster(cx).map_or(0, |poster| poster.start());
            self.showing(cx);
        } else {
            cx.emit(StatusClosed);
        }
    }

    /// Goes back to the previous person's statuses.
    fn previous_poster(&mut self, cx: &mut Context<Self>) {
        if self.at_poster > 0 {
            self.at_poster -= 1;
            self.at_post = self.poster(cx).map_or(0, |poster| poster.start());
            self.showing(cx);
        }
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        if self.at_post > 0 {
            self.at_post -= 1;
        } else if self.at_poster > 0 {
            self.at_poster -= 1;
            self.at_post = self
                .poster(cx)
                .map_or(0, |poster| poster.posts.len().saturating_sub(1));
        }
        self.showing(cx);
    }

    fn hold(&mut self, held: bool) {
        match (held, self.held) {
            (true, None) => self.held = Some(Instant::now()),
            (false, Some(since)) if self.paused.is_none() => {
                self.started += since.elapsed();
                self.held = None;
            }
            _ => {}
        }
    }

    /// How long the post on screen shows.
    fn length(post: &Post) -> Duration {
        match post.kind {
            ContentKind::Video => post
                .media
                .as_ref()
                .and_then(|media| media.seconds)
                .map_or(VIDEO, |seconds| Duration::from_secs(seconds.into())),
            _ => STILL,
        }
    }

    /// The share of the post on screen already shown, 0 to 1.
    fn progress(&self, post: &Post) -> f32 {
        let playing = self.player.as_ref().is_some_and(|(id, _)| *id == post.id);
        if post.kind == ContentKind::Video && !playing {
            return 0.0;
        }
        let shown = match self.held {
            Some(since) => since.duration_since(self.started),
            None => self.started.elapsed(),
        };
        (shown.as_secs_f32() / Self::length(post).as_secs_f32()).clamp(0.0, 1.0)
    }

    fn on_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => cx.emit(StatusClosed),
            "left" => self.back(cx),
            "right" => self.forward(cx),
            "up" => self.next_poster(cx),
            "down" => self.previous_poster(cx),
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Render for StatusViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        // A video's player takes the keyboard and drops it when the next
        // post replaces it; the arrows must keep reaching the viewer.
        if !self.focus.contains_focused(window, cx) {
            window.focus(&self.focus, cx);
        }
        let Some(poster) = self.poster(cx) else {
            return div().into_any_element();
        };
        let Some(post) = poster.posts.get(self.at_post).cloned() else {
            return div().into_any_element();
        };
        let progress = self.progress(&post);
        if progress >= 1.0 && self.held.is_none() && !self.advancing {
            // Moves on after this draw, not during it.
            self.advancing = true;
            cx.defer_in(window, |this, _, cx| {
                this.advancing = false;
                this.forward(cx);
            });
        }
        if self.held.is_none() {
            window.request_animation_frame();
        }
        let viewport = window.viewport_size();
        let title = if self.mine {
            "My status".into()
        } else {
            self.book.read(cx).title(&poster.poster)
        };
        let picture = self
            .pictures
            .update(cx, |pictures, cx| pictures.picture(&poster.poster, cx));
        let layer = div()
            .id("status-viewer")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event, _, cx| this.on_key(event, cx)))
            // Before a video's player, which pauses itself on the same key
            // and keeps it from reaching here.
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "space" && !this.reply.focus_handle(cx).is_focused(window)
                {
                    if this.paused.take().is_some() {
                        this.hold(false);
                    } else {
                        this.hold(true);
                        this.paused = Some(Instant::now());
                    }
                    cx.notify();
                }
            }))
            .w(viewport.width)
            .h(viewport.height)
            .flex()
            .flex_col()
            .bg(rgb(theme.deep))
            .text_color(rgb(theme.text))
            .occlude()
            .child(self.top(&poster, &post, progress, title, picture, cx))
            .child(self.stage(&post, cx))
            .child(self.bottom(&post, cx));
        deferred(anchored().position(point(px(0.0), px(0.0))).child(layer))
            .with_priority(2)
            .into_any_element()
    }
}

impl StatusViewer {
    fn top(
        &self,
        poster: &Poster,
        post: &Post,
        progress: f32,
        title: SharedString,
        picture: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = *cx.theme();
        let bars = poster.posts.iter().enumerate().map(|(index, _)| {
            let filled = match index.cmp(&self.at_post) {
                std::cmp::Ordering::Less => 1.0,
                std::cmp::Ordering::Equal => progress,
                std::cmp::Ordering::Greater => 0.0,
            };
            div()
                .flex_1()
                .h(px(3.0))
                .rounded_full()
                .bg(rgba((theme.text << 8) | 0x40))
                .child(
                    div()
                        .h_full()
                        .w(relative(filled))
                        .rounded_full()
                        .bg(rgb(theme.text)),
                )
        });
        div()
            .flex_none()
            .px_6()
            .pt_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().flex().gap_1().children(bars))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(avatar(poster.poster.as_str(), &title, picture, px(36.0)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().font_weight(FontWeight::MEDIUM).child(title))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_size(px(12.5))
                                    .text_color(rgb(theme.muted))
                                    .child(when(post.sent_at))
                                    .children(post.link.clone().map(link_button)),
                            ),
                    )
                    .child(
                        Button::new("close-status")
                            .icon(Icon::Close)
                            .variant(ButtonVariant::Ghost)
                            .round()
                            .tooltip("Close (Esc)")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(StatusClosed))),
                    ),
            )
    }
}

/// Where the status leads, as the phone offers it under the poster's
/// name: a button when it opens something, else a note.
fn link_button(link: StatusLink) -> AnyElement {
    match link.url {
        Some(url) => Button::new("status-link")
            .label(link.label)
            .icon(Icon::ChevronRight)
            .variant(ButtonVariant::Ghost)
            .on_click(move |_, _, cx| cx.open_url(&url))
            .into_any_element(),
        None => div().child(link.label).into_any_element(),
    }
}
