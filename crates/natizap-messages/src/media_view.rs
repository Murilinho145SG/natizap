//! How attachments look inside a bubble.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, Div, FontWeight, ImageSource, MouseButton, MouseDownEvent, ObjectFit,
    Pixels, WeakEntity, canvas, div, img, px, relative, rgb, svg,
};
use natizap_protocol::ContentKind;
use natizap_ui::Icon;
use natizap_ui::theme::{DEEP, HONEY, HONEY_INK, MUTED, TEXT};

use crate::items::MediaView;
use crate::view::ConversationView;
use crate::voice::VoiceState;

/// Bars a waveform is drawn with.
const BARS: usize = 36;

/// The attachment of a bubble: an image for photos and stickers, a preview
/// with a play button for videos, a player for voice notes and audio, and a
/// row for documents. Videos play over the window, sound plays in the
/// bubble, and documents open in the system's app for them.
pub(crate) fn render_media(
    media: &MediaView,
    voice: Option<&VoiceState>,
    view: &WeakEntity<ConversationView>,
) -> AnyElement {
    match media.fetch.kind {
        ContentKind::Photo | ContentKind::Sticker => picture(media).into_any_element(),
        ContentKind::Video => picture(media)
            .relative()
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(round_icon(Icon::Play, 48.0)),
            )
            .children(media.detail.clone().map(|duration| {
                div()
                    .absolute()
                    .bottom_1()
                    .left_2()
                    .text_size(px(12.0))
                    .text_color(rgb(TEXT))
                    .child(duration)
            }))
            .id("media")
            .cursor_pointer()
            .on_click(open(media, view))
            .into_any_element(),
        ContentKind::VoiceMessage | ContentKind::Audio => {
            let voice = voice.filter(|voice| voice.message_id == media.fetch.message_id.as_str());
            render_voice(media, voice, view).into_any_element()
        }
        kind => div()
            .id("media")
            .min_w(px(220.0))
            .flex()
            .items_center()
            .gap_3()
            .p_2()
            .rounded(px(10.0))
            .bg(rgb(DEEP))
            .cursor_pointer()
            .on_click(open(media, view))
            .child(round_icon(
                if kind == ContentKind::Document {
                    Icon::File
                } else {
                    Icon::Play
                },
                36.0,
            ))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().font_weight(FontWeight::MEDIUM).child(kind.label()))
                    .children(media.detail.clone().map(|detail| {
                        div()
                            .truncate()
                            .text_size(px(12.5))
                            .text_color(rgb(MUTED))
                            .child(detail)
                    })),
            )
            .into_any_element(),
    }
}

/// The downloaded image, else the inline preview, else an empty frame. A
/// video's file is not an image, so videos always show their preview.
fn picture(media: &MediaView) -> Div {
    let (width, height) = media.size;
    let is_image = matches!(media.fetch.kind, ContentKind::Photo | ContentKind::Sticker);
    let source: Option<ImageSource> = match (&media.file, &media.preview) {
        (Some(file), _) if is_image && file.exists() => Some(file.clone().into()),
        (_, Some(preview)) => Some(preview.clone().into()),
        _ => None,
    };
    div()
        .w(px(width))
        .h(px(height))
        .rounded(px(10.0))
        .overflow_hidden()
        .when(media.fetch.kind != ContentKind::Sticker, |frame| {
            frame.bg(rgb(DEEP))
        })
        .children(source.map(|source| {
            let fit = if media.fetch.kind == ContentKind::Sticker {
                ObjectFit::Contain
            } else {
                ObjectFit::Cover
            };
            img(source).size_full().object_fit(fit)
        }))
}

/// Play button, waveform, time and, while it is the note playing, speed.
fn render_voice(
    media: &MediaView,
    voice: Option<&VoiceState>,
    view: &WeakEntity<ConversationView>,
) -> Div {
    let playing = voice.is_some_and(|voice| voice.playing);
    let progress = voice.map_or(0.0, |voice| voice.progress);
    let time = voice.map_or_else(
        || media.detail.clone().unwrap_or_default(),
        |voice| clock_label(voice.position).into(),
    );
    let message_id = media.fetch.message_id.clone();
    let track: Rc<Cell<Option<Bounds<Pixels>>>> = Rc::default();
    let seek = {
        let (track, view) = (Rc::clone(&track), view.clone());
        move |event: &MouseDownEvent, _: &mut gpui::Window, cx: &mut App| {
            let Some(bounds) = track.get() else { return };
            let fraction = (event.position.x - bounds.origin.x) / bounds.size.width;
            // The view is gone only while the window closes.
            let _ = view.update(cx, |view, cx| view.seek_voice(&message_id, fraction, cx));
            cx.stop_propagation();
        }
    };
    let toggle = {
        let (media, view) = (media.clone(), view.clone());
        move |_: &gpui::ClickEvent, _: &mut gpui::Window, cx: &mut App| {
            let _ = view.update(cx, |view, cx| view.toggle_voice(&media, cx));
        }
    };
    let waveform = media
        .fetch
        .media
        .waveform
        .as_deref()
        .filter(|levels| !levels.is_empty());

    div()
        .w(px(250.0))
        .flex()
        .items_center()
        .gap_3()
        .pt_1()
        .child(voice_button(
            playing,
            voice.is_some_and(|voice| voice.loading),
            toggle,
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .id("waveform")
                        .relative()
                        .h(px(28.0))
                        .flex()
                        .items_center()
                        .justify_between()
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, seek)
                        .child(
                            canvas(
                                move |bounds, _, _| track.set(Some(bounds)),
                                |_, (), _, _| {},
                            )
                            .absolute()
                            .size_full(),
                        )
                        .map(|row| match waveform {
                            Some(levels) => row.children(bars(levels, progress)),
                            None => row.child(plain_track(progress)),
                        })
                        .child(
                            div()
                                .absolute()
                                .top(px(8.0))
                                .left(relative(progress))
                                .ml(px(-6.0))
                                .size(px(12.0))
                                .rounded_full()
                                .bg(rgb(TEXT)),
                        ),
                )
                .child(
                    div()
                        .h(px(18.0))
                        .flex()
                        .items_center()
                        .justify_between()
                        .text_size(px(12.0))
                        .text_color(rgb(MUTED))
                        .child(time)
                        .children(voice.map(|voice| speed_chip(voice, view))),
                ),
        )
}

/// The honey play/pause button, dimmed while the file downloads.
fn voice_button(
    playing: bool,
    loading: bool,
    toggle: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id("voice-toggle")
        .size(px(40.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(rgb(HONEY))
        .cursor_pointer()
        .hover(|button| button.opacity(0.9))
        .when(loading, |button| button.opacity(0.5))
        .on_click(toggle)
        // Nudged right: a triangle's visual center sits left of its box's.
        .child(
            div().when(!playing, |glyph| glyph.pl(px(2.0))).child(
                svg()
                    .path(if playing { Icon::Pause } else { Icon::Play }.path())
                    .size(px(18.0))
                    .text_color(rgb(HONEY_INK)),
            ),
        )
}

/// Shows the speed and switches to the next one on click.
fn speed_chip(voice: &VoiceState, view: &WeakEntity<ConversationView>) -> impl IntoElement {
    let view = view.clone();
    div()
        .id("voice-speed")
        .px_1p5()
        .rounded_full()
        .bg(rgb(DEEP))
        .text_color(rgb(TEXT))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            // The view is gone only while the window closes.
            let _ = view.update(cx, ConversationView::cycle_voice_speed);
        })
        .child(voice.speed.label())
}

/// The waveform as bars; those before `progress` are lit.
#[expect(
    clippy::cast_precision_loss,
    reason = "bar counts and loudness values are tiny"
)]
fn bars(levels: &[u8], progress: f32) -> Vec<Div> {
    let peaks = peaks(levels, BARS);
    let count = peaks.len() as f32;
    peaks
        .into_iter()
        .enumerate()
        .map(|(index, level)| {
            let lit = (index as f32 + 0.5) / count <= progress;
            div()
                .w(px(3.0))
                .h(px(4.0 + f32::from(level) * 0.2))
                .rounded_full()
                .bg(if lit {
                    rgb(TEXT)
                } else {
                    rgb(MUTED).opacity(0.45)
                })
        })
        .collect()
}

/// A plain slider for notes sent without a waveform.
fn plain_track(progress: f32) -> Div {
    div()
        .w_full()
        .h(px(3.0))
        .rounded_full()
        .bg(rgb(MUTED).opacity(0.45))
        .child(
            div()
                .h_full()
                .w(relative(progress))
                .rounded_full()
                .bg(rgb(TEXT)),
        )
}

/// Squeezes `levels` (0 to 100) into at most `count` bars, keeping each
/// group's loudest sample so short peaks of speech stay visible.
fn peaks(levels: &[u8], count: usize) -> Vec<u8> {
    let count = count.min(levels.len());
    (0..count)
        .map(|bar| {
            let start = bar * levels.len() / count;
            let end = ((bar + 1) * levels.len() / count).max(start + 1);
            levels[start..end]
                .iter()
                .copied()
                .max()
                .unwrap_or(0)
                .min(100)
        })
        .collect()
}

fn clock_label(time: Duration) -> String {
    let seconds = time.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn round_icon(icon: Icon, size: f32) -> Div {
    div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(rgb(0x00_00_00).opacity(0.5))
        .child(
            svg()
                .path(icon.path())
                .size(px(size * 0.45))
                .text_color(rgb(TEXT)),
        )
}

fn open(
    media: &MediaView,
    view: &WeakEntity<ConversationView>,
) -> impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut App) + 'static {
    let (media, view) = (media.clone(), view.clone());
    move |_, _, cx| {
        // The view is gone only while the window closes.
        let _ = view.update(cx, |view, cx| view.open_media(&media, cx));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squeezing_a_waveform_keeps_its_peaks() {
        let mut levels = [10; 64];
        levels[5] = 90;
        let bars = peaks(&levels, 36);
        assert_eq!(bars.len(), 36);
        assert_eq!(bars.iter().max(), Some(&90));
    }

    #[test]
    fn a_short_waveform_is_not_stretched() {
        assert_eq!(peaks(&[1, 2, 3], 36), [1, 2, 3]);
        assert_eq!(peaks(&[200], 36), [100]);
    }
}
