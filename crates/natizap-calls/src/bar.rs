//! A call on screen inside the main window: the bar across its top while a
//! call is under way, and the card over it while one rings.

use gpui::prelude::*;
use gpui::{Context, Entity, EventEmitter, FontWeight, Window, div, px, rgb};
use natizap_profiles::ProfilePictures;
use natizap_ui::theme::{ActiveTheme as _, DISPLAY_FONT, Theme};
use natizap_ui::{Button, ButtonSize, ButtonVariant, Icon, avatar};

use crate::desk::{CallDesk, CallShown, CameraState, Phase};
use crate::pane::CallChatRequested;

/// The user asked for the call in a window of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallPoppedOut;

/// The call under way, as a bar across the top of the window: who, how
/// long, and its controls. Draws nothing without a call.
pub struct CallBar {
    desk: Entity<CallDesk>,
    pictures: Entity<ProfilePictures>,
}

impl std::fmt::Debug for CallBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallBar").finish_non_exhaustive()
    }
}

impl EventEmitter<CallChatRequested> for CallBar {}
impl EventEmitter<CallPoppedOut> for CallBar {}

impl CallBar {
    /// Shows `desk`'s call.
    pub fn new(
        desk: Entity<CallDesk>,
        pictures: Entity<ProfilePictures>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&desk, |_, _, cx| cx.notify()).detach();
        cx.observe(&pictures, |_, _, cx| cx.notify()).detach();
        Self { desk, pictures }
    }
}

impl Render for CallBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let Some(call) = self.desk.read(cx).call(cx) else {
            return div().into_any_element();
        };
        let picture = self
            .pictures
            .update(cx, |pictures, cx| pictures.picture(&call.peer, cx));
        let ended = matches!(call.phase, Phase::Ended(_));
        let peer = call.peer.clone();
        div()
            .h(px(52.0))
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .bg(rgb(theme.raised))
            .border_b_1()
            .border_color(rgb(theme.line))
            .child(
                div()
                    .id("call-bar-peer")
                    .flex()
                    .items_center()
                    .gap_3()
                    .min_w_0()
                    .cursor_pointer()
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(CallChatRequested(peer.clone()));
                    }))
                    .child(avatar(call.peer.as_str(), &call.title, picture, px(32.0)))
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(call.title.clone()),
                            )
                            .child(phase_line(&call, theme)),
                    ),
            )
            .child(
                div()
                    .ml_auto()
                    .flex()
                    .items_center()
                    .gap_1()
                    .when(!ended, |controls| {
                        controls
                            .when(call.video_requested, |controls| {
                                controls.child(video_request(cx))
                            })
                            .child(mute_button(
                                &call,
                                ButtonSize::Medium,
                                cx.listener(|bar, _, _, cx| {
                                    bar.desk.update(cx, CallDesk::toggle_muted);
                                }),
                            ))
                            .child(camera_button(
                                &call,
                                ButtonSize::Medium,
                                cx.listener(|bar, _, _, cx| {
                                    bar.desk.update(cx, CallDesk::toggle_camera);
                                }),
                            ))
                            .child(
                                Button::new("call-pop-out")
                                    .icon(Icon::PopOut)
                                    .variant(ButtonVariant::Ghost)
                                    .round()
                                    .tooltip("Open in its own window")
                                    .on_click(cx.listener(|_, _, _, cx| cx.emit(CallPoppedOut))),
                            )
                            .child(
                                Button::new("call-hang-up")
                                    .icon(Icon::CallEnd)
                                    .variant(ButtonVariant::Danger)
                                    .round()
                                    .tooltip("Hang up")
                                    .on_click(cx.listener(|bar, _, _, cx| {
                                        bar.desk.update(cx, CallDesk::hang_up);
                                    })),
                            )
                    }),
            )
            .into_any_element()
    }
}

/// The call ringing here, as a card floating over the window's top right
/// corner. Draws nothing while nothing rings.
pub struct RingCard {
    desk: Entity<CallDesk>,
    pictures: Entity<ProfilePictures>,
}

impl std::fmt::Debug for RingCard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RingCard").finish_non_exhaustive()
    }
}

impl RingCard {
    /// Shows the calls ringing on `desk`.
    pub fn new(
        desk: Entity<CallDesk>,
        pictures: Entity<ProfilePictures>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&desk, |_, _, cx| cx.notify()).detach();
        cx.observe(&pictures, |_, _, cx| cx.notify()).detach();
        Self { desk, pictures }
    }
}

impl Render for RingCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let Some(ring) = self.desk.read(cx).ringing().cloned() else {
            return div().into_any_element();
        };
        let title = self.desk.read(cx).title(&ring.caller, cx);
        let in_call = self.desk.read(cx).call(cx).is_some();
        let picture = self
            .pictures
            .update(cx, |pictures, cx| pictures.picture(&ring.caller, cx));
        let kind = if ring.video {
            "Video call"
        } else {
            "Voice call"
        };
        div()
            .absolute()
            .top_3()
            .right_3()
            .w(px(320.0))
            .occlude()
            .child(
                theme
                    .glass(div())
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .rounded_xl()
                    .bg(rgb(theme.raised))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(avatar(ring.caller.as_str(), &title, picture, px(48.0)))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .truncate()
                                            .font_family(DISPLAY_FONT)
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_size(px(17.0))
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .text_color(rgb(theme.muted))
                                            .child(kind),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                div().flex_1().child(
                                    Button::new("ring-decline")
                                        .icon(Icon::CallEnd)
                                        .label("Decline")
                                        .variant(ButtonVariant::Danger)
                                        .on_click(cx.listener(|card, _, _, cx| {
                                            card.desk.update(cx, CallDesk::decline);
                                        })),
                                ),
                            )
                            .child(
                                div().flex_1().child(
                                    Button::new("ring-answer")
                                        .icon(if ring.video { Icon::Video } else { Icon::Call })
                                        .label(if in_call { "End and answer" } else { "Answer" })
                                        .on_click(cx.listener(|card, _, _, cx| {
                                            card.desk.update(cx, CallDesk::answer);
                                        })),
                                ),
                            ),
                    ),
            )
            .into_any_element()
    }
}

/// What the call is doing, in a line: its clock once it is live.
pub(crate) fn phase_line(call: &CallShown, theme: Theme) -> impl IntoElement + use<> {
    let (text, color) = match &call.phase {
        Phase::Starting | Phase::Calling => ("Calling…".to_owned(), theme.muted),
        Phase::Ringing => ("Ringing…".to_owned(), theme.muted),
        Phase::Connecting => ("Connecting…".to_owned(), theme.muted),
        Phase::Live { since } => (clock(since.elapsed().as_secs()), theme.mint),
        Phase::Ended(end) => (ended(end), theme.alert),
    };
    div()
        .flex()
        .items_center()
        .gap_1p5()
        .text_size(px(12.5))
        .text_color(rgb(color))
        .when(call.muted, |line| {
            line.child(
                gpui::svg()
                    .path(Icon::MicOff.path())
                    .size(px(13.0))
                    .text_color(rgb(theme.muted)),
            )
        })
        .child(text)
}

/// The microphone's switch.
pub(crate) fn mute_button(
    call: &CallShown,
    size: ButtonSize,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> Button {
    Button::new("call-mute")
        .icon(if call.muted { Icon::MicOff } else { Icon::Mic })
        .variant(if call.muted {
            ButtonVariant::Secondary
        } else {
            ButtonVariant::Ghost
        })
        .size(size)
        .selected(call.muted)
        .round()
        .tooltip(if call.muted { "Unmute" } else { "Mute" })
        .on_click(on_click)
}

/// The camera's switch: turning it on makes a voice call a video call.
pub(crate) fn camera_button(
    call: &CallShown,
    size: ButtonSize,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> Button {
    let camera = call.video.as_ref().map(|video| video.camera);
    let on = matches!(camera, Some(CameraState::On | CameraState::Opening));
    Button::new("call-camera")
        .icon(if on { Icon::Video } else { Icon::VideoOff })
        .variant(if on {
            ButtonVariant::Secondary
        } else {
            ButtonVariant::Ghost
        })
        .size(size)
        .selected(on)
        .round()
        .tooltip(match camera {
            Some(CameraState::Missing) => "No camera found",
            _ if on => "Turn camera off",
            _ => "Turn camera on",
        })
        .on_click(on_click)
}

/// The other end's request to turn the call into a video call, with the
/// answers to it.
fn video_request(cx: &mut Context<CallBar>) -> impl IntoElement + use<> {
    let theme = *cx.theme();
    div()
        .mr_2()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .text_size(px(12.5))
                .text_color(rgb(theme.muted))
                .child("Wants to turn on video"),
        )
        .child(
            Button::new("call-video-accept")
                .icon(Icon::Video)
                .label("Turn on")
                .size(ButtonSize::Small)
                .on_click(cx.listener(|bar, _, _, cx| {
                    bar.desk.update(cx, CallDesk::accept_video);
                })),
        )
        .child(
            Button::new("call-video-ignore")
                .label("Not now")
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Small)
                .on_click(cx.listener(|bar, _, _, cx| {
                    bar.desk.update(cx, CallDesk::ignore_video);
                })),
        )
}

/// How long a call has gone: `4:05`, or `1:02:09` past an hour.
fn clock(seconds: u64) -> String {
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn ended(end: &natizap_protocol::CallEnd) -> String {
    use natizap_protocol::CallEnd;
    match end {
        CallEnd::Finished => "Call ended".to_owned(),
        CallEnd::Declined => "Declined".to_owned(),
        CallEnd::Unanswered => "No answer".to_owned(),
        CallEnd::Failed(why) => format!("Call failed: {why}"),
    }
}

#[cfg(test)]
mod tests {
    use super::clock;

    #[test]
    fn the_clock_shows_hours_only_past_one() {
        assert_eq!(clock(0), "0:00");
        assert_eq!(clock(245), "4:05");
        assert_eq!(clock(3_729), "1:02:09");
    }
}
