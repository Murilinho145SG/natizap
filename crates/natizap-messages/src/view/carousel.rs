//! The open photo or video over the whole window, with the chat's other
//! photos and videos a step away: the arrow keys or the side buttons walk
//! through them, and a strip of small pictures shows where it is.

use std::path::PathBuf;

use chrono::{Local, TimeZone as _};
use gpui::prelude::*;
use gpui::{
    AnyElement, AnyView, Context, Div, EventEmitter, ImageSource, KeyDownEvent, MouseButton,
    MouseDownEvent, ObjectFit, SharedString, Window, anchored, deferred, div, img, point, px, rgb,
    svg,
};
use natizap_protocol::{ChatId, ContentKind};
use natizap_ui::motion::{fade_in, slide_in};
use natizap_ui::theme::{DEEP, MUTED, TEXT};
use natizap_ui::{Button, ButtonSize, ButtonVariant, Icon, Menu};

use super::ConversationView;
use crate::carousel::{Carousel, Moved, Toward};
use crate::items::MediaView;
use crate::library::Library;
use crate::thumbs;
use crate::zoom::Zoom;

mod strip;
mod zoom;

/// Height of the bar over the stage.
const TOP_BAR: f32 = 56.0;
/// Room the side buttons take on each side of the stage.
const SIDE: f32 = 72.0;
/// How far an item slides in from the side it comes from.
const SLIDE: f32 = 80.0;

/// The carousel landed on a downloaded video; the app answers with a
/// player through [`ConversationView::show_player`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayRequested {
    /// The message the video came in, to hand the player back with.
    pub message_id: SharedString,
    /// The downloaded file.
    pub file: PathBuf,
}

impl EventEmitter<PlayRequested> for ConversationView {}

impl ConversationView {
    /// Opens `media` in the carousel, among the open chat's photos and
    /// videos.
    pub(crate) fn view_media(&mut self, media: &MediaView, cx: &mut Context<Self>) {
        let chats = self
            .log
            .read(cx)
            .open_chat()
            .map_or_default(|chat| chat.addresses.clone());
        self.open_carousel(media, chats, cx);
    }

    /// Opens `media` in the carousel, among the photos and videos of
    /// `chats`.
    pub(super) fn open_carousel(
        &mut self,
        media: &MediaView,
        chats: Vec<ChatId>,
        cx: &mut Context<Self>,
    ) {
        let library = self.log.read(cx).library(chats.clone(), cx);
        let reading = cx.spawn(async move |this, cx| {
            let library = library.await;
            let _ = this.update(cx, |this, cx| this.shelve(library, cx));
        });
        self.carousel = Some(Carousel::new(
            media.clone(),
            chats,
            cx.focus_handle(),
            reading,
        ));
        self.settle_carousel(cx);
        cx.notify();
    }

    /// Takes the chat's photos and videos as read, and makes the small
    /// copies the strip lacks.
    fn shelve(&mut self, library: Library, cx: &mut Context<Self>) {
        let Some(carousel) = self.carousel.as_mut() else {
            return;
        };
        let wanted = library.small_copies_wanted();
        carousel.shelve(library.media);
        carousel.making = cx.spawn(async move |this, cx| {
            thumbs::make_in_batches(wanted, cx, |made, cx| {
                this.update(cx, |this, cx| {
                    if let Some(carousel) = this.carousel.as_mut() {
                        carousel.made.extend(made);
                    }
                    cx.notify();
                })
                .is_ok()
            })
            .await;
        });
        strip::reveal(carousel);
        cx.notify();
    }

    /// Moves the carousel one item `toward`, if there is one.
    pub(super) fn step_carousel(&mut self, toward: Toward, cx: &mut Context<Self>) {
        if self
            .carousel
            .as_mut()
            .is_some_and(|carousel| carousel.step(toward))
        {
            self.carousel_moved(cx);
        }
    }

    /// Moves the carousel to the item at `index` on its shelf.
    pub(super) fn jump_carousel(&mut self, index: usize, cx: &mut Context<Self>) {
        if self
            .carousel
            .as_mut()
            .is_some_and(|carousel| carousel.go(index))
        {
            self.carousel_moved(cx);
        }
    }

    fn carousel_moved(&mut self, cx: &mut Context<Self>) {
        if let Some(carousel) = &self.carousel {
            strip::reveal(carousel);
        }
        self.settle_carousel(cx);
        cx.notify();
    }

    /// Gets what the carousel shows ready: downloads it if it is not here,
    /// and asks the app for a player once a video is.
    pub(super) fn settle_carousel(&mut self, cx: &mut Context<Self>) {
        let Some(carousel) = self.carousel.as_mut() else {
            return;
        };
        let media = carousel.current().clone();
        let log = self.log.read(cx);
        let file = on_disk(&media).or_else(|| log.saved_file(&media.fetch));
        match file {
            None => {
                let fetch = media.fetch.clone();
                self.log.update(cx, |log, cx| log.download(fetch, cx));
            }
            Some(file) if media.fetch.kind == ContentKind::Video => {
                let id = media.fetch.message_id.clone();
                if carousel.asked.as_deref() != Some(id.as_str()) {
                    carousel.asked = Some(id.clone());
                    cx.emit(PlayRequested {
                        message_id: id.into(),
                        file,
                    });
                }
            }
            Some(_) => {}
        }
    }

    /// Plays `player`, the app's answer to a [`PlayRequested`] for the
    /// video of message `message_id`, if the carousel still shows it.
    pub fn show_player(&mut self, message_id: &str, player: AnyView, cx: &mut Context<Self>) {
        if let Some(carousel) = self.carousel.as_mut() {
            carousel.play(message_id, player);
            cx.notify();
        }
    }

    pub(super) fn close_carousel(&mut self, cx: &mut Context<Self>) {
        if self.carousel.take().is_some() {
            self.refocus = true;
            cx.notify();
        }
    }

    fn carousel_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let toward = match event.keystroke.key.as_str() {
            "left" => Toward::Older,
            "right" => Toward::Newer,
            // Escape first brings a zoomed photo back to fit.
            "escape" | "0" => {
                if let Some(carousel) = self.carousel.as_mut().filter(|c| c.zoom.zoomed()) {
                    carousel.zoom = Zoom::default();
                    cx.stop_propagation();
                    cx.notify();
                }
                return;
            }
            _ => return,
        };
        self.step_carousel(toward, cx);
        cx.stop_propagation();
    }

    /// The carousel over the whole window, when open.
    pub(super) fn carousel_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let carousel = self.carousel.as_ref()?;
        // It takes the keyboard as it opens, and back when the video
        // player that had it goes with the video.
        if !carousel.focus.contains_focused(window, cx) {
            window.focus(&carousel.focus, cx);
        }
        let viewport = window.viewport_size();
        let layer = div()
            .id("carousel")
            .track_focus(&carousel.focus)
            .on_key_down(cx.listener(|this, event, _, cx| this.carousel_key(event, cx)))
            .w(viewport.width)
            .h(viewport.height)
            .flex()
            .flex_col()
            .bg(rgb(DEEP))
            .text_color(rgb(TEXT))
            .occlude()
            .child(Self::carousel_bar(carousel, cx))
            .child(self.carousel_stage(carousel, window, cx))
            .child(strip::strip(carousel, cx))
            .children(Self::carousel_menu(carousel, cx));
        Some(
            deferred(
                anchored()
                    .position(point(px(0.0), px(0.0)))
                    .child(fade_in("carousel-in", layer)),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// Where it is among the chat's photos and videos and when it was
    /// sent, then what can be done with it.
    fn carousel_bar(carousel: &Carousel, cx: &mut Context<Self>) -> Div {
        let media = carousel.current().clone();
        let items = carousel.shelf();
        let place = carousel.at().map(|at| {
            let sent = items
                .get(at)
                .and_then(|item| Local.timestamp_opt(item.sent_at, 0).single())
                .map_or_else(String::new, |sent| {
                    sent.format(" · %-d %b %Y, %H:%M").to_string()
                });
            format!("{} of {}{sent}", items.len() - at, items.len())
        });
        let photo = media.fetch.kind == ContentKind::Photo;
        let going_to = carousel.at().and_then(|at| items.get(at)).map(|item| {
            (
                SharedString::from(item.media.fetch.message_id.clone()),
                item.sent_at,
                carousel.chats.clone(),
            )
        });
        div()
            .flex_none()
            .h(px(TOP_BAR))
            .px_4()
            .flex()
            .items_center()
            .gap_1()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.5))
                    .text_color(rgb(MUTED))
                    .children(place),
            )
            .when(photo, |bar| {
                let media = media.clone();
                bar.child(
                    bar_button("carousel-copy", Icon::Copy, "Copy image").on_click(
                        cx.listener(move |this, _, _, cx| this.copy_media_image(&media, cx)),
                    ),
                )
            })
            .child({
                let media = media.clone();
                bar_button("carousel-save", Icon::Download, "Save as…")
                    .on_click(cx.listener(move |_, _, _, cx| Self::save_media_as(&media, cx)))
            })
            .children(going_to.map(|(id, sent_at, chats)| {
                bar_button("carousel-go", Icon::Chats, "Go to message").on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.close_carousel(cx);
                        this.go_to(id.clone(), sent_at, chats.clone(), cx);
                    },
                ))
            }))
            .child(
                bar_button("carousel-close", Icon::Close, "Close (Esc)")
                    .on_click(cx.listener(|this, _, _, cx| this.close_carousel(cx))),
            )
    }

    /// What shows, as large as the stage allows, sliding in from the side
    /// it came from, between the buttons to step to its neighbors.
    fn carousel_stage(
        &self,
        carousel: &Carousel,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let media = carousel.current();
        let id = SharedString::from(format!("carousel-{}", media.fetch.message_id));
        let content = match carousel.player() {
            Some(player) => div()
                .size_full()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(player.clone()),
            None => self.still(carousel, media, window, cx),
        };
        let content = content.id(id.clone()).on_mouse_down(
            MouseButton::Right,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                if let Some(carousel) = this.carousel.as_mut() {
                    carousel.menu = Some(event.position);
                }
                cx.stop_propagation();
                cx.notify();
            }),
        );
        let content = match carousel.moved {
            Moved::Opened => fade_in(id, content).into_any_element(),
            Moved::Older => slide_in(id, content, -SLIDE).into_any_element(),
            Moved::Newer => slide_in(id, content, SLIDE).into_any_element(),
        };
        div()
            .id("carousel-stage")
            .flex_1()
            .min_h_0()
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .px(px(SIDE))
            // A click beside what shows closes it, as on the dimmed window
            // a photo used to open over.
            .on_click(cx.listener(|this, _, _, cx| this.close_carousel(cx)))
            .child(content)
            .child(side_button(carousel, Toward::Older, cx))
            .child(side_button(carousel, Toward::Newer, cx))
    }

    /// A photo, or a video's still while it downloads or its player comes.
    fn still(
        &self,
        carousel: &Carousel,
        media: &MediaView,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let video = media.fetch.kind == ContentKind::Video;
        let file = on_disk(media).or_else(|| self.log.read(cx).saved_file(&media.fetch));
        let source: Option<ImageSource> = match file {
            Some(file) if !video => Some(file.into()),
            _ => media
                .poster
                .clone()
                .filter(|poster| poster.exists())
                .map(Into::into)
                .or_else(|| media.preview.clone().map(Into::into)),
        };
        let geometry = zoom::Geometry::of(media, window.viewport_size());
        if let (Some(source), false) = (&source, video) {
            return zoom::photo(source.clone(), carousel.zoom, geometry, cx);
        }
        let frame = div()
            .relative()
            .w(px(geometry.fit.x))
            .h(px(geometry.fit.y))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .flex()
            .items_center()
            .justify_center();
        let frame = match source {
            Some(source) => frame.child(
                img(source)
                    .size_full()
                    .rounded(px(8.0))
                    .object_fit(ObjectFit::Contain),
            ),
            None => frame.child(
                svg()
                    .path(if video { Icon::Video } else { Icon::Camera }.path())
                    .size(px(40.0))
                    .text_color(rgb(MUTED)),
            ),
        };
        // A video waits here for its download or its player.
        frame.when(video, |frame| {
            frame.child(
                div()
                    .absolute()
                    .bottom_3()
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .bg(rgb(DEEP))
                    .text_size(px(12.5))
                    .text_color(rgb(MUTED))
                    .child("Loading…"),
            )
        })
    }

    /// Copy image and Save as, where a right click on what shows landed.
    fn carousel_menu(carousel: &Carousel, cx: &mut Context<Self>) -> Option<AnyElement> {
        let position = carousel.menu?;
        let media = carousel.current().clone();
        let view = cx.weak_entity();
        let mut menu = Menu::new();
        if media.fetch.kind == ContentKind::Photo {
            let (view, media) = (view.clone(), media.clone());
            menu = menu.action(Icon::Copy, "Copy image", move |_, cx| {
                let _ = view.update(cx, |this, cx| {
                    close_menu(this);
                    this.copy_media_image(&media, cx);
                });
            });
        }
        let saving = view.clone();
        let menu = menu
            .action(Icon::Download, "Save as…", move |_, cx| {
                let _ = saving.update(cx, |this, cx| {
                    close_menu(this);
                    Self::save_media_as(&media, cx);
                });
            })
            .on_dismiss(move |_, cx| {
                let _ = view.update(cx, |this, cx| {
                    close_menu(this);
                    cx.notify();
                });
            });
        Some(
            deferred(
                anchored()
                    .position(position)
                    .snap_to_window_with_margin(px(8.0))
                    .child(menu),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
}

fn close_menu(view: &mut ConversationView) {
    if let Some(carousel) = view.carousel.as_mut() {
        carousel.menu = None;
    }
}

/// `media`'s file, when it is on disk.
fn on_disk(media: &MediaView) -> Option<PathBuf> {
    media.file.clone().filter(|file| file.exists())
}

fn bar_button(id: &'static str, icon: Icon, tooltip: &'static str) -> Button {
    Button::new(id)
        .icon(icon)
        .variant(ButtonVariant::Ghost)
        .round()
        .tooltip(tooltip)
}

/// The button stepping `toward` a neighbor, at its side of the stage;
/// nothing at the end of the chat's photos and videos.
fn side_button(carousel: &Carousel, toward: Toward, cx: &mut Context<ConversationView>) -> Div {
    let (id, icon, tooltip) = match toward {
        Toward::Older => ("carousel-older", Icon::ChevronLeft, "Previous (←)"),
        Toward::Newer => ("carousel-newer", Icon::ChevronRight, "Next (→)"),
    };
    let side = div().absolute().top_0().bottom_0().flex().items_center();
    let side = match toward {
        Toward::Older => side.left_4(),
        Toward::Newer => side.right_4(),
    };
    side.when(carousel.can_step(toward), |side| {
        side.child(
            Button::new(id)
                .icon(icon)
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Large)
                .round()
                .tooltip(tooltip)
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.step_carousel(toward, cx);
                })),
        )
    })
}
