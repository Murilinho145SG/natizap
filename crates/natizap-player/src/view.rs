use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    Bounds, Context, Corners, Div, FocusHandle, Focusable, KeyDownEvent, MouseButton, ObjectFit,
    Pixels, RenderImage, Window, canvas, div, px, rgb, svg,
};
use image::{Frame, RgbaImage};
use natizap_playback::Playback;
use natizap_ui::theme::{LINE, MUTED, TEXT};
use natizap_ui::{Button, ButtonSize, Icon, progress};

/// Plays a video, with its own decoder and sound output: nothing outside
/// natizap is opened. It fills the element it is placed in; whoever shows
/// it owns closing it and moving on, so it answers only Space.
pub struct PlayerView {
    playback: Playback,
    picture: Option<Arc<RenderImage>>,
    /// Pictures replaced since the last frame, to free from the GPU atlas.
    retired: Vec<Arc<RenderImage>>,
    paused: bool,
    focus: FocusHandle,
    track: Rc<Cell<Option<Bounds<Pixels>>>>,
}

impl std::fmt::Debug for PlayerView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerView").finish_non_exhaustive()
    }
}

impl Focusable for PlayerView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PlayerView {
    /// Starts playing `path`, taking the keyboard so Space pauses it.
    /// Playback stops when the view is dropped.
    pub fn new(path: &Path, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            playback: Playback::start(path),
            picture: None,
            retired: Vec::new(),
            paused: false,
            focus,
            track: Rc::default(),
        }
    }

    fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        if self.playback.status().ended {
            self.seek(Duration::ZERO, cx);
            self.paused = false;
        } else {
            self.paused = !self.paused;
        }
        self.playback.set_paused(self.paused);
        cx.notify();
    }

    fn seek(&mut self, to: Duration, cx: &mut Context<Self>) {
        self.playback.seek(to);
        cx.notify();
    }

    /// Takes the newest picture whose time has come.
    fn advance(&mut self) {
        let Some(picture) = self.playback.due_picture() else {
            return;
        };
        let Some(buffer) = RgbaImage::from_raw(picture.width, picture.height, picture.bgra) else {
            return;
        };
        let image = Arc::new(RenderImage::new(vec![Frame::new(buffer)]));
        if let Some(old) = self.picture.replace(image) {
            self.retired.push(old);
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if event.keystroke.key == "space" {
            self.toggle_pause(cx);
            cx.stop_propagation();
        }
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
        let status = self.playback.status();
        let position = self.playback.position();
        if !self.paused && !status.ended && status.error.is_none() {
            window.request_animation_frame();
        }

        let stage = match (&status.error, &self.picture, status.has_video) {
            (Some(error), ..) => notice(format!("Could not play this file: {error}")),
            (None, Some(picture), true) => frame(picture.clone()),
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
            .text_color(rgb(TEXT))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p_4()
                    .child(div().size_full().relative().child(stage)),
            )
            .child(self.controls(position, status.duration, status.ended, cx))
    }
}

impl PlayerView {
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
                Button::new("play-pause")
                    .icon(if playing { Icon::Pause } else { Icon::Play })
                    .size(ButtonSize::Large)
                    .round()
                    .tooltip(if playing {
                        "Pause (Space)"
                    } else {
                        "Play (Space)"
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_pause(cx))),
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
                        progress(fraction).relative().child(
                            canvas(
                                move |bounds, _, _| track.set(Some(bounds)),
                                |_, (), _, _| {},
                            )
                            .absolute()
                            .size_full(),
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

/// The video's picture, as large as the stage allows with its shape kept.
///
/// Painted by hand: an image element sized to the stage in relative units
/// is laid out by the picture's own shape, so an upright video grew far
/// taller than the stage and showed cropped, as if zoomed in.
fn frame(picture: Arc<RenderImage>) -> Div {
    div().absolute().inset_0().child(
        canvas(
            |_, _, _| {},
            move |bounds, (), window, _| {
                let fitted = ObjectFit::Contain.get_bounds(bounds, picture.size(0));
                if let Err(error) = window.paint_image(
                    fitted,
                    fitted,
                    Corners::default(),
                    Arc::clone(&picture),
                    0,
                    false,
                ) {
                    tracing::debug!(%error, "could not draw a video frame");
                }
            },
        )
        .size_full(),
    )
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
