//! How a voice note or audio file looks inside its bubble: an accent play
//! button in a dark ring, a progress track with a knob that can be dragged,
//! a speed menu, and the time played beside the length.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt as _, App, Bounds, Div, FontWeight, MouseButton, MouseDownEvent,
    MouseMoveEvent, Pixels, Point, SharedString, WeakEntity, Window, bounce, canvas, deferred, div,
    ease_in_out, px, relative, rgb, svg,
};
use natizap_playback::Speed;
use natizap_ui::theme::{ACCENT, ACCENT_INK, DEEP, LINE, MUTED, PANEL, RAISED, TEXT, glass};
use natizap_ui::{Icon, progress};

use crate::items::MediaView;
use crate::view::ConversationView;
use crate::voice::VoiceUi;

/// Where the track is on screen, measured while painting so clicks and
/// drags can tell how far along they are.
type TrackBounds = Rc<Cell<Option<Bounds<Pixels>>>>;

/// The player of `media`'s note, ending in `footer`: the message's time
/// and ticks. `ui` says which note plays, at what speed, and whose speed
/// menu is open.
pub(crate) fn render_voice(
    media: &MediaView,
    ui: &VoiceUi,
    downloading: Option<f32>,
    footer: impl IntoElement,
    view: &WeakEntity<ConversationView>,
) -> Div {
    let id = SharedString::from(media.fetch.message_id.clone());
    let voice = ui.playing.as_ref().filter(|voice| voice.message_id == id);
    // While it downloads, the track shows how far the file came.
    let progress = downloading.unwrap_or_else(|| voice.map_or(0.0, |voice| voice.progress));
    // Time played while this note is the one playing, its length otherwise,
    // as on the phone.
    let downloaded = media.file.is_some();
    let time: SharedString = if let Some(voice) = voice {
        clock_label(voice.position).into()
    } else {
        let length = media.detail.clone().unwrap_or_default();
        // Not here yet: how much it takes to fetch, as on the phone.
        match media.fetch.media.size.filter(|_| !downloaded) {
            Some(bytes) => format!("{length} · {}", file_size(bytes)).into(),
            None => length,
        }
    };
    let state = match voice {
        _ if downloading.is_some() => Button::Downloading,
        Some(voice) if voice.loading => Button::Downloading,
        Some(voice) if voice.playing => Button::Pause,
        _ if !downloaded => Button::Download,
        _ => Button::Play,
    };
    let toggle = {
        let (media, view) = (media.clone(), view.clone());
        move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
            // The view is gone only while the window closes.
            let _ = view.update(cx, |view, cx| view.toggle_voice(&media, cx));
        }
    };

    div()
        .w(px(340.0))
        .max_w_full()
        .flex()
        .items_center()
        .gap_2p5()
        .child(play_button(state, toggle))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(track(&id, progress, view))
                        .child(speed_menu(&id, ui, view)),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .text_size(px(12.0))
                        .child(
                            svg()
                                .path(Icon::Waveform.path())
                                .size(px(14.0))
                                .text_color(rgb(MUTED)),
                        )
                        .child(div().text_color(rgb(TEXT)).child(time))
                        .child(div().flex_1())
                        .child(footer),
                ),
        )
}

/// What the note's button shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    /// The file is not here: a click fetches it, then plays it.
    Download,
    /// Fetching it, to play once it lands.
    Downloading,
    Play,
    Pause,
}

/// The accent button in its dark ring: download until the file is here,
/// pulsing while it comes, then play and pause.
fn play_button(
    state: Button,
    toggle: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let icon = match state {
        Button::Download | Button::Downloading => Icon::Download,
        Button::Play => Icon::Play,
        Button::Pause => Icon::Pause,
    };
    let glyph = svg()
        .path(icon.path())
        .size(px(14.0))
        .text_color(rgb(ACCENT_INK));
    let glyph = if state == Button::Downloading {
        glyph
            .with_animation(
                "voice-downloading",
                Animation::new(Duration::from_millis(900))
                    .repeat()
                    .with_easing(bounce(ease_in_out)),
                |glyph, delta| glyph.opacity(1.0 - 0.6 * delta),
            )
            .into_any_element()
    } else {
        glyph.into_any_element()
    };
    div()
        .size(px(40.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(rgb(DEEP).opacity(0.55))
        .child(
            div()
                .id("voice-toggle")
                .size(px(32.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(rgb(ACCENT))
                .cursor_pointer()
                .hover(|button| button.opacity(0.9))
                .on_click(toggle)
                .child(glyph),
        )
}

/// The progress track: accent up to the knob. Clicking or dragging along it
/// jumps there.
fn track(
    id: &SharedString,
    fraction: f32,
    view: &WeakEntity<ConversationView>,
) -> impl IntoElement {
    let bounds: TrackBounds = Rc::default();
    let measure = Rc::clone(&bounds);
    let pressed = {
        let (bounds, id, view) = (Rc::clone(&bounds), id.clone(), view.clone());
        move |event: &MouseDownEvent, _: &mut Window, cx: &mut App| {
            seek_to(&bounds, event.position, &id, &view, cx);
            cx.stop_propagation();
        }
    };
    let dragged = {
        let (id, view) = (id.clone(), view.clone());
        move |event: &MouseMoveEvent, _: &mut Window, cx: &mut App| {
            if event.pressed_button == Some(MouseButton::Left) {
                seek_to(&bounds, event.position, &id, &view, cx);
            }
        }
    };

    div()
        .id("voice-track")
        .flex_1()
        .min_w_0()
        .h(px(18.0))
        .relative()
        .flex()
        .items_center()
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, pressed)
        .on_mouse_move(dragged)
        .child(
            canvas(move |area, _, _| measure.set(Some(area)), |_, (), _, _| {})
                .absolute()
                .size_full(),
        )
        .child(progress(fraction))
        .child(
            div()
                .absolute()
                .top(px(3.0))
                .left(relative(fraction))
                .ml(px(-6.0))
                .size(px(12.0))
                .rounded_full()
                .bg(rgb(TEXT))
                .shadow_sm(),
        )
}

fn seek_to(
    bounds: &TrackBounds,
    position: Point<Pixels>,
    id: &SharedString,
    view: &WeakEntity<ConversationView>,
    cx: &mut App,
) {
    let Some(bounds) = bounds.get() else { return };
    let fraction = (position.x - bounds.origin.x) / bounds.size.width;
    // The view is gone only while the window closes.
    let _ = view.update(cx, |view, cx| view.seek_voice(id, fraction, cx));
}

/// The speed pill and, when open, the list of speeds floating above it.
fn speed_menu(id: &SharedString, ui: &VoiceUi, view: &WeakEntity<ConversationView>) -> Div {
    let open = ui.speed_menu.as_ref() == Some(id);
    let toggle = {
        let (id, view) = (id.clone(), view.clone());
        move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
            let _ = view.update(cx, |view, cx| view.toggle_speed_menu(&id, cx));
        }
    };
    div()
        .relative()
        .flex_none()
        .child(
            div()
                .id("voice-speed")
                .flex()
                .items_center()
                .gap_0p5()
                .pl_2()
                .pr_1p5()
                .py_0p5()
                .rounded_full()
                .bg(rgb(DEEP))
                .border_1()
                .border_color(rgb(LINE))
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(TEXT))
                .cursor_pointer()
                .hover(|pill| pill.border_color(rgb(MUTED)))
                .on_click(toggle)
                .child(ui.speed.label())
                .child(
                    svg()
                        .path(Icon::ChevronDown.path())
                        .size(px(12.0))
                        .text_color(rgb(MUTED)),
                ),
        )
        .when(open, |pill| {
            let close = view.clone();
            // Clicks on the pill toggle the menu themselves, so only clicks
            // outside it close the menu here.
            pill.on_mouse_down_out(move |_, _, cx| {
                let _ = close.update(cx, ConversationView::close_popups);
            })
            .child(speed_list(ui.speed, view))
        })
}

/// Floats above the pill so it stays clear of the composer below the last
/// message.
fn speed_list(current: Speed, view: &WeakEntity<ConversationView>) -> impl IntoElement {
    deferred(
        div()
            .absolute()
            .bottom(relative(1.0))
            .right_0()
            .mb_1()
            .w(px(80.0))
            .p_1()
            .flex()
            .flex_col()
            .rounded_lg()
            .bg(rgb(PANEL))
            .map(glass)
            .occlude()
            .children(Speed::ALL.into_iter().map(|speed| {
                let view = view.clone();
                div()
                    .id(speed.label())
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .text_size(px(12.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(if speed == current { ACCENT } else { TEXT }))
                    .cursor_pointer()
                    .hover(|row| row.bg(rgb(RAISED)))
                    // On press, not click: the press already closed the menu as
                    // a click outside the pill, and the release would land on
                    // nothing.
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let _ = view.update(cx, |view, cx| view.set_voice_speed(speed, cx));
                        cx.stop_propagation();
                    })
                    .child(speed.label())
            })),
    )
}

/// A file's size as the phone writes it: "85 KB", "1.2 MB".
#[expect(
    clippy::cast_precision_loss,
    reason = "a voice note's size is far below f64's exact range"
)]
fn file_size(bytes: u64) -> String {
    let kilobytes = bytes as f64 / 1024.0;
    if kilobytes < 1024.0 {
        format!("{} KB", kilobytes.ceil())
    } else {
        format!("{:.1} MB", kilobytes / 1024.0)
    }
}

fn clock_label(time: Duration) -> String {
    let seconds = time.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_sizes_as_the_phone_does() {
        assert_eq!(file_size(87_000), "85 KB");
        assert_eq!(file_size(1_258_291), "1.2 MB");
    }

    #[test]
    fn labels_the_time_played_in_minutes_and_seconds() {
        assert_eq!(clock_label(Duration::from_millis(31_900)), "0:31");
        assert_eq!(clock_label(Duration::from_secs(84)), "1:24");
    }
}
