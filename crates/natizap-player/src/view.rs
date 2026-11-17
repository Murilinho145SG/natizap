use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    Bounds, Context, Div, EventEmitter, FocusHandle, Focusable, FontWeight, KeyDownEvent,
    MouseButton, ObjectFit, Pixels, RenderImage, SharedString, Window, canvas, div, img, px,
    relative, rgb, svg,
};
use image::{Frame, RgbaImage};
use natizap_ui::Icon;
use natizap_ui::theme::{DEEP, HONEY, HONEY_INK, LINE, MUTED, TEXT};

use crate::decode::{self, Command, Picture, Shared};

/// How far the arrow keys jump.
const ARROW_SEEK: Duration = Duration::from_secs(5);

/// The user closed the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerClosed;

/// Plays a video or audio file over the window, with its own decoder and
/// sound output: nothing outside natizap is opened.
pub struct PlayerView {
    title: SharedString,
    shared: Arc<Shared>,
    commands: mpsc::Sender<Command>,
    picture: Option<Arc<RenderImage>>,
    /// Pictures replaced since the last frame, to free from the GPU atlas.
    retired: Vec<Arc<RenderImage>>,
    paused: bool,
    focus: FocusHandle,
    track: Rc<Cell<Option<Bounds<Pixels>>>>,
}

impl std::fmt::Debug for PlayerView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerView")
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

impl EventEmitter<PlayerClosed> for PlayerView {}

impl Focusable for PlayerView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PlayerView {
    /// Starts playing `path`, shown under `title`. Playback stops when the
    /// view is dropped.
    pub fn new(
        path: &Path,
        title: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let shared = Arc::new(Shared::default());
        let (commands, received) = mpsc::channel();
        let (decoding, path) = (Arc::clone(&shared), path.to_owned());
        // A plain thread: decoding blocks on FFmpeg and must never share a
        // pool with UI or protocol work. Dropping `commands` ends it.
        if let Err(error) = std::thread::Builder::new()
            .name("natizap-player".into())
            .spawn(move || decode::run(&path, &decoding, &received))
        {
            shared.status().error = Some(error.to_string());
        }
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            title,
            shared,
            commands,
            picture: None,
            retired: Vec::new(),
            paused: false,
            focus,
            track: Rc::default(),
        }
    }

    fn send(&self, command: Command) {
        // Only fails once the decoder stopped, when there is nothing to steer.
        let _ = self.commands.send(command);
    }

    fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        if self.shared.status().ended {
            self.seek(Duration::ZERO, cx);
            self.paused = false;
        } else {
            self.paused = !self.paused;
        }
        self.send(Command::Pause(self.paused));
        cx.notify();
    }

    fn seek(&mut self, to: Duration, cx: &mut Context<Self>) {
        let to = self
            .shared
            .status()
            .duration
            .map_or(to, |duration| to.min(duration));
        self.send(Command::Seek(to));
        cx.notify();
    }

    fn seek_by(&mut self, delta: Duration, forward: bool, cx: &mut Context<Self>) {
        let now = self.shared.clock.position();
        let to = if forward {
            now + delta
        } else {
            now.saturating_sub(delta)
        };
        self.seek(to, cx);
    }

    fn close(cx: &mut Context<Self>) {
        cx.emit(PlayerClosed);
    }

    /// Takes the newest picture whose time has come.
    fn advance(&mut self) {
        let now = self.shared.clock.position();
        let mut due: Option<Picture> = None;
        {
            let mut pictures = self.shared.pictures();
            while pictures.front().is_some_and(|picture| picture.at <= now) {
                due = pictures.pop_front();
            }
        }
        let Some(picture) = due else { return };
        let Some(buffer) = RgbaImage::from_raw(picture.width, picture.height, picture.bgra) else {
            return;
        };
        let image = Arc::new(RenderImage::new(vec![Frame::new(buffer)]));
        if let Some(old) = self.picture.replace(image) {
            self.retired.push(old);
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "space" => self.toggle_pause(cx),
            "escape" => Self::close(cx),
            "left" => self.seek_by(ARROW_SEEK, false, cx),
            "right" => self.seek_by(ARROW_SEEK, true, cx),
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Render for PlayerView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.advance();
        for image in self.retired.drain(..) {
            if let Err(error) = window.drop_image(image) {
                tracing::debug!(%error, "could not free a video frame");
            }
        }
        let status = self.shared.status().clone();
        let position = self.shared.clock.position();
        if !self.paused && !status.ended && status.error.is_none() {
            window.request_animation_frame();
        }

        let stage = match (&status.error, &self.picture, status.has_video) {
            (Some(error), ..) => notice(format!("Could not play this file: {error}")),
            (None, Some(picture), true) => div().size_full().child(
                img(picture.clone())
                    .size_full()
                    .object_fit(ObjectFit::Contain),
            ),
            (None, None, true) => notice("Loading…".to_owned()),
            (None, _, false) => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    svg()
                        .path(Icon::Play.path())
                        .size(px(96.0))
                        .text_color(rgb(LINE)),
                ),
        };

        div()
            .id("player")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event, _, cx| this.on_key(event, cx)))
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(DEEP))
            .text_color(rgb(TEXT))
            .child(self.top_bar(cx))
            .child(div().flex_1().min_h_0().p_4().child(stage))
            .child(self.controls(position, status.duration, status.ended, cx))
    }
}

impl PlayerView {
    fn top_bar(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex_none()
            .flex()
            .items_center()
            .px_4()
            .py_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::MEDIUM)
                    .child(self.title.clone()),
            )
            .child(
                div()
                    .id("close-player")
                    .size(px(32.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(LINE)))
                    .on_click(cx.listener(|_, _, _, cx| Self::close(cx)))
                    .child(
                        svg()
                            .path(Icon::Close.path())
                            .size(px(18.0))
                            .text_color(rgb(MUTED)),
                    ),
            )
    }

    fn controls(
        &self,
        position: Duration,
        duration: Option<Duration>,
        ended: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let total = duration.unwrap_or(Duration::ZERO);
        let fraction = if total.is_zero() {
            0.0
        } else {
            (position.as_secs_f32() / total.as_secs_f32()).clamp(0.0, 1.0)
        };
        let playing = !self.paused && !ended;
        let track = Rc::clone(&self.track);
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap_4()
            .px_6()
            .py_4()
            .child(
                div()
                    .id("play-pause")
                    .size(px(44.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(rgb(HONEY))
                    .cursor_pointer()
                    .hover(|style| style.opacity(0.9))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_pause(cx)))
                    .child(if playing { pause_glyph() } else { play_glyph() }),
            )
            .child(
                div()
                    .text_size(px(13.0))
                    .text_color(rgb(MUTED))
                    .child(clock_label(position.min(total.max(position)))),
            )
            .child(
                div()
                    .id("progress")
                    .flex_1()
                    .h(px(20.0))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                            let Some(bounds) = this.track.get() else {
                                return;
                            };
                            let x = (event.position.x - bounds.origin.x) / bounds.size.width;
                            this.seek(total.mul_f32(x.clamp(0.0, 1.0)), cx);
                        }),
                    )
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(px(4.0))
                            .rounded_full()
                            .bg(rgb(LINE))
                            .child(
                                canvas(
                                    move |bounds, _, _| track.set(Some(bounds)),
                                    |_, (), _, _| {},
                                )
                                .absolute()
                                .size_full(),
                            )
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(fraction))
                                    .rounded_full()
                                    .bg(rgb(HONEY)),
                            ),
                    ),
            )
            .child(
                div()
                    .text_size(px(13.0))
                    .text_color(rgb(MUTED))
                    .child(clock_label(total)),
            )
    }
}

fn notice(message: String) -> Div {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_color(rgb(MUTED))
        .child(message)
}

fn play_glyph() -> Div {
    // Nudged right: a triangle's visual center sits left of its box's.
    div().pl(px(2.0)).child(
        svg()
            .path(Icon::Play.path())
            .size(px(20.0))
            .text_color(rgb(HONEY_INK)),
    )
}

/// Two bars, drawn rather than an icon file: it is the only place a pause
/// symbol appears.
fn pause_glyph() -> Div {
    let bar = || div().w(px(4.0)).h(px(16.0)).rounded_sm().bg(rgb(HONEY_INK));
    div().flex().gap(px(4.0)).child(bar()).child(bar())
}

fn clock_label(time: Duration) -> String {
    let seconds = time.as_secs();
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_times_like_a_media_player() {
        assert_eq!(clock_label(Duration::from_secs(7)), "0:07");
        assert_eq!(clock_label(Duration::from_secs(125)), "2:05");
        assert_eq!(clock_label(Duration::from_secs(3725)), "1:02:05");
    }
}
