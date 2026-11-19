//! The menu a right click on the other end of a call opens, as on Discord:
//! how loud they play, from silent to twice as loud, and muting them for
//! the user alone.

use gpui::prelude::*;
use gpui::{
    App, Context, Entity, FontWeight, MouseButton, Pixels, Point, Window, anchored, deferred, div,
    px, rgb, rgba, svg,
};
use natizap_ui::theme::{ActiveTheme as _, Theme};
use natizap_ui::{Icon, Slider, Switch, Tooltip};

use crate::desk::{CallDesk, CallShown};
use crate::volume::{Listening, Loudness, PeerMicrophone};

/// The menu `view` opened at `at`, for `call`'s other end; closing it
/// empties the place `slot` finds in the view.
pub(crate) fn person_menu<V: 'static>(
    cx: &Context<V>,
    slot: fn(&mut V) -> &mut Option<Point<Pixels>>,
    desk: &Entity<CallDesk>,
    call: &CallShown,
    at: Point<Pixels>,
) -> gpui::AnyElement {
    let theme = *cx.theme();
    let view = cx.weak_entity();
    let dismiss = move |_: &mut Window, cx: &mut App| {
        let _ = view.update(cx, |view, cx| {
            *slot(view) = None;
            cx.notify();
        });
    };
    let loudness = call.peer_loudness;
    let muted = call.listening == Listening::Muted;
    let louder = desk.clone();
    let muting = desk.clone();
    deferred(
        anchored()
            .position(at)
            .snap_to_window_with_margin(px(8.0))
            .child(
                div()
                    .id("call-person-menu")
                    .w(px(248.0))
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .rounded_lg()
                    .bg(rgb(theme.panel))
                    .text_color(rgb(theme.text))
                    .map(|surface| theme.glass(surface))
                    .occlude()
                    .on_mouse_down_out(move |_, window, cx| dismiss(window, cx))
                    // A right click inside keeps it open where it is.
                    .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(call.title.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .text_size(px(13.0))
                                    .child(div().text_color(rgb(theme.muted)).child("Volume"))
                                    .child(format!("{}%", loudness.percent())),
                            )
                            .child(
                                Slider::new("call-person-volume", loudness.fraction())
                                    .mark(Loudness::SENT.fraction())
                                    .on_change(move |fraction, _, cx| {
                                        louder.update(cx, |desk, cx| {
                                            desk.set_peer_loudness(Loudness::at(fraction), cx);
                                        });
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                svg()
                                    .path(Icon::Muted.path())
                                    .size(px(15.0))
                                    .text_color(rgb(theme.muted)),
                            )
                            .child(div().flex_1().text_size(px(13.5)).child("Mute for me"))
                            .child(Switch::new("call-person-mute", muted).on_toggle(
                                move |_, _, cx| {
                                    muting.update(cx, CallDesk::toggle_peer_muted);
                                },
                            )),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(rgb(theme.muted))
                            .child("Only you hear the change: they are not told."),
                    ),
            ),
    )
    .with_priority(1)
    .into_any_element()
}

/// The marks on the other end's picture or name: their microphone off,
/// which they chose, and muted for the user, which the user chose, each
/// its own mark. None while neither is so.
pub(crate) fn peer_marks(call: &CallShown, theme: Theme) -> Option<impl IntoElement + use<>> {
    let off = call.peer_microphone == PeerMicrophone::Off;
    let muted = call.listening == Listening::Muted;
    (off || muted).then(|| {
        div()
            .flex()
            .items_center()
            .gap_1()
            .when(off, |marks| {
                marks.child(mark(
                    "call-peer-microphone-off",
                    Icon::MicOff,
                    theme.text,
                    "Their microphone is off",
                ))
            })
            .when(muted, |marks| {
                marks.child(mark(
                    "call-peer-muted",
                    Icon::Muted,
                    theme.alert,
                    "Muted for you",
                ))
            })
    })
}

/// The marks on a dark pill, to lay over the other end's picture.
pub(crate) fn peer_badge(call: &CallShown, theme: Theme) -> Option<gpui::Div> {
    peer_marks(call, theme).map(|marks| {
        div()
            .px_2()
            .py_1()
            .rounded_full()
            .bg(rgba(theme.scrim))
            .child(marks)
    })
}

/// The other end's state in words, under their picture in a voice call.
pub(crate) fn peer_notes(call: &CallShown, theme: Theme) -> impl IntoElement + use<> {
    let note = |icon: Icon, color: u32, text: &'static str| {
        div()
            .flex()
            .items_center()
            .gap_1()
            .text_size(px(13.0))
            .text_color(rgb(color))
            .child(
                svg()
                    .path(icon.path())
                    .size(px(14.0))
                    .flex_none()
                    .text_color(rgb(color)),
            )
            .child(text)
    };
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap_1()
        .when(call.peer_microphone == PeerMicrophone::Off, |notes| {
            notes.child(note(Icon::MicOff, theme.muted, "Their microphone is off"))
        })
        .when(call.listening == Listening::Muted, |notes| {
            notes.child(note(Icon::Muted, theme.alert, "Muted for you"))
        })
}

/// One mark, with what it means on hover.
fn mark(id: &'static str, icon: Icon, color: u32, meaning: &'static str) -> impl IntoElement {
    div().id(id).tooltip(Tooltip::text(meaning)).child(
        svg()
            .path(icon.path())
            .size(px(14.0))
            .flex_none()
            .text_color(rgb(color)),
    )
}
