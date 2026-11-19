//! A call on screen inside the main window: the bar across its top while a
//! call is under way, and the card over it while one rings.

use gpui::prelude::*;
use gpui::{Context, Entity, EventEmitter, FontWeight, SharedString, Window, div, px, rgb};
use natizap_profiles::ProfilePictures;
use natizap_ui::motion::rise_in;
use natizap_ui::theme::{ActiveTheme as _, DISPLAY_FONT, Theme};
use natizap_ui::{Button, ButtonSize, ButtonVariant, Icon, avatar};

use crate::desk::{CallDesk, CallLayout, CallShown, CameraState, Phase, Sharing};
use crate::keys::{CAMERA, HANG_UP, MUTE};
use crate::look::{quality_bars, quality_word, ringed};
use crate::pane::CallChatRequested;
use crate::quality::Quality;

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
        let stage_shown = matches!(
            self.desk.read(cx).layout(),
            CallLayout::Docked | CallLayout::Filled
        );
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
                    .child(ringed(
                        "call-bar-speaking",
                        avatar(call.peer.as_str(), &call.title, picture, px(32.0)),
                        32.0,
                        call.speaking,
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
                                    .child(call.title.clone()),
                            )
                            .child(phase_line(&call, theme)),
                    ),
            )
            // A call docked or filling the chat's area has its controls on
            // its stage.
            .when(!ended && !stage_shown, |bar| {
                bar.child(self.controls(&call, cx))
            })
            .into_any_element()
    }
}

impl CallBar {
    /// The call's controls, at the bar's right end.
    fn controls(&self, call: &CallShown, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let layout = self.desk.read(cx).layout();
        let call = call.clone();
        div()
            .ml_auto()
            .flex()
            .items_center()
            .gap_1()
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
            .child(screen_button(
                &call,
                ButtonSize::Medium,
                cx.listener(|bar, _, _, cx| {
                    bar.desk.update(cx, CallDesk::toggle_screen);
                }),
            ))
            .child(show_button(layout, cx))
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
                    .tooltip(HANG_UP.tip("Hang up"))
                    .on_click(cx.listener(|bar, _, _, cx| {
                        bar.desk.update(cx, CallDesk::hang_up);
                    })),
            )
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
            .child(rise_in(
                SharedString::from(format!("ring-card-{}", ring.id)),
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
                            .child(ringed(
                                SharedString::from(format!("ring-pulse-{}", ring.id)),
                                avatar(ring.caller.as_str(), &title, picture, px(48.0)),
                                48.0,
                                true,
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
                -12.0,
            ))
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
        .when_some(
            call.quality
                .filter(|_| matches!(call.phase, Phase::Live { .. })),
            |line, quality| {
                line.child(div().ml_1().child(quality_bars(quality, theme)))
                    // A good line needs no words; a bad one is worth knowing.
                    .when(quality != Quality::Good, |line| {
                        line.child(
                            div()
                                .text_color(rgb(theme.muted))
                                .child(quality_word(quality)),
                        )
                    })
            },
        )
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
        .tooltip(MUTE.tip(if call.muted { "Unmute" } else { "Mute" }))
        .on_click(on_click)
}

/// The button that shows the call over the chat, or only in the bar.
fn show_button(layout: CallLayout, cx: &mut Context<CallBar>) -> Button {
    let (icon, tip, to) = if layout == CallLayout::Bar {
        (
            Icon::ChevronDown,
            "Show the call over the chat",
            CallLayout::Docked,
        )
    } else {
        (Icon::ChevronUp, "Show only the bar", CallLayout::Bar)
    };
    Button::new("call-show")
        .icon(icon)
        .variant(ButtonVariant::Ghost)
        .round()
        .tooltip(tip)
        .on_click(cx.listener(move |bar, _, _, cx| {
            bar.desk.update(cx, |desk, cx| desk.set_layout(to, cx));
        }))
}

/// The screen's switch: sharing it needs no camera, and turns a voice
/// call into a video call.
pub(crate) fn screen_button(
    call: &CallShown,
    size: ButtonSize,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> Button {
    let sharing = call
        .video
        .as_ref()
        .map_or(Sharing::Off, |video| video.sharing);
    let on = sharing != Sharing::Off;
    Button::new("call-screen")
        .icon(Icon::ScreenShare)
        .variant(if on {
            ButtonVariant::Secondary
        } else {
            ButtonVariant::Ghost
        })
        .size(size)
        .selected(on)
        .round()
        .tooltip(match sharing {
            Sharing::Off => "Share your screen",
            Sharing::Choosing => "Choosing what to share…",
            Sharing::On => "Stop sharing your screen",
        })
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
            Some(CameraState::Missing) => "No camera found".to_owned(),
            _ if on => CAMERA.tip("Turn camera off"),
            _ => CAMERA.tip("Turn camera on"),
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
