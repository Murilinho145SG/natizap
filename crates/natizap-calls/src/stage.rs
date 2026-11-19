//! The call shown large: over the chat in the main window, docked with the
//! chat below it or filling the chat's area, and in a window of its own. A
//! voice call shows the person in the middle, with the call's clock under
//! them in the call's own window (the main window's bar shows it there);
//! a video call shows its pictures as tiles (see `tiles`). The controls sit at the bottom; in the main
//! window, its arrangement's buttons sit at the top right, and a docked
//! stage's foot is dragged to make it taller or shorter.

use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    Bounds, Context, Entity, EventEmitter, FontWeight, MouseButton, MouseDownEvent, Pixels, Point,
    Window, div, px, rgb, rgba,
};
use natizap_profiles::ProfilePictures;
use natizap_ui::theme::{ActiveTheme as _, DISPLAY_FONT, Theme};
use natizap_ui::{Button, ButtonSize, ButtonVariant, Icon, avatar};

use crate::bar::{CallPoppedOut, camera_button, mute_button, phase_line, screen_button};
use crate::desk::{CallDesk, CallLayout, CallShown, Phase};
use crate::keys::HANG_UP;
use crate::look::ringed;
use crate::pointer::{Held, follower};
use crate::tiles::{Arrangement, Grab};

mod video;

/// The shortest a docked call gets: its controls and the person still fit.
const LEAST_HEIGHT: f32 = 200.0;
/// What a docked call leaves of the chat below it, at least.
const LEAST_CHAT: f32 = 160.0;

/// Where a stage shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Home {
    /// The main window, where the call's arrangement can change.
    Main,
    /// The call's own window.
    Window,
}

/// What the pointer holds down on a stage.
#[derive(Debug, Clone, Copy)]
enum Hold {
    /// A docked stage's foot: where it went down, and the height then.
    Foot(Point<Pixels>, Pixels),
    /// A small tile.
    Tile(Grab),
}

/// The call under way, shown large.
pub struct CallStage {
    desk: Entity<CallDesk>,
    pictures: Entity<ProfilePictures>,
    home: Home,
    paints: video::Paints,
    /// How the user arranged a video call's tiles.
    arrangement: Arrangement,
    /// The small tiles beside the focused one, as last drawn.
    small_count: usize,
    /// What the pointer holds down.
    held: Option<Hold>,
    /// The stage, as last painted.
    area: Rc<Cell<Bounds<Pixels>>>,
    /// The tallest a docked stage may get in the window as last drawn.
    tallest: Pixels,
}

impl std::fmt::Debug for CallStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallStage")
            .field("home", &self.home)
            .finish_non_exhaustive()
    }
}

impl EventEmitter<CallPoppedOut> for CallStage {}

impl CallStage {
    /// Shows `desk`'s call large in the main window.
    pub fn new(
        desk: Entity<CallDesk>,
        pictures: Entity<ProfilePictures>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::at(Home::Main, desk, pictures, cx)
    }

    pub(crate) fn at(
        home: Home,
        desk: Entity<CallDesk>,
        pictures: Entity<ProfilePictures>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&desk, |_, _, cx| cx.notify()).detach();
        cx.observe(&pictures, |_, _, cx| cx.notify()).detach();
        Self {
            desk,
            pictures,
            home,
            paints: video::Paints::default(),
            arrangement: Arrangement::default(),
            small_count: 0,
            held: None,
            area: Rc::default(),
            tallest: px(f32::MAX),
        }
    }

    /// The person large in the middle, while no picture of theirs shows.
    fn person(&self, call: &CallShown, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = *cx.theme();
        let picture = self
            .pictures
            .update(cx, |pictures, cx| pictures.picture(&call.peer, cx));
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .px_6()
            .child(ringed(
                "call-stage-speaking",
                avatar(call.peer.as_str(), &call.title, picture, px(96.0)),
                96.0,
                call.speaking,
                theme,
            ))
            // The main window's bar already says who and how long.
            .when(self.home == Home::Window, |person| {
                person
                    .child(
                        div()
                            .pt_1()
                            .max_w_full()
                            .truncate()
                            .font_family(DISPLAY_FONT)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(22.0))
                            .child(call.title.clone()),
                    )
                    .child(phase_line(call, theme))
            })
    }

    /// The call's controls, in a row.
    fn controls(call: &CallShown, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(mute_button(
                call,
                ButtonSize::Large,
                cx.listener(|this, _, _, cx| this.desk.update(cx, CallDesk::toggle_muted)),
            ))
            .child(camera_button(
                call,
                ButtonSize::Large,
                cx.listener(|this, _, _, cx| this.desk.update(cx, CallDesk::toggle_camera)),
            ))
            .child(screen_button(
                call,
                ButtonSize::Large,
                cx.listener(|this, _, _, cx| this.desk.update(cx, CallDesk::toggle_screen)),
            ))
            .child(
                Button::new("call-stage-hang-up")
                    .icon(Icon::CallEnd)
                    .variant(ButtonVariant::Danger)
                    .size(ButtonSize::Large)
                    .round()
                    .tooltip(HANG_UP.tip("Hang up"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.desk.update(cx, CallDesk::hang_up);
                    })),
            )
    }

    /// The buttons that arrange the call in the main window.
    fn arrangement(layout: CallLayout, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let arrange = |id: &'static str, icon: Icon, tip: &'static str, to: CallLayout| {
            Button::new(id)
                .icon(icon)
                .variant(ButtonVariant::Ghost)
                .round()
                .tooltip(tip)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.desk.update(cx, |desk, cx| desk.set_layout(to, cx));
                }))
        };
        let fill = if layout == CallLayout::Filled {
            arrange(
                "call-stage-dock",
                Icon::Restore,
                "Show the chat",
                CallLayout::Docked,
            )
        } else {
            arrange(
                "call-stage-fill",
                Icon::Maximize,
                "Fill the chat's area",
                CallLayout::Filled,
            )
        };
        div()
            .absolute()
            .top_2()
            .right_2()
            .flex()
            .gap_1()
            .p_1()
            .rounded_full()
            .bg(rgba(cx.theme().scrim))
            .child(fill)
            .child(arrange(
                "call-stage-float",
                Icon::Minimize,
                "Float as a small picture",
                CallLayout::Floating,
            ))
            .child(arrange(
                "call-stage-hide",
                Icon::ChevronUp,
                "Show only the bar",
                CallLayout::Bar,
            ))
            .child(
                Button::new("call-stage-pop-out")
                    .icon(Icon::PopOut)
                    .variant(ButtonVariant::Ghost)
                    .round()
                    .tooltip("Open in its own window")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(CallPoppedOut))),
            )
    }

    /// A docked stage's foot, dragged to make the stage taller or shorter.
    fn foot(theme: Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .id("call-stage-foot")
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .h(px(6.0))
            .cursor_row_resize()
            .border_b_1()
            .border_color(rgb(theme.line))
            .hover(|foot| foot.bg(rgba(theme.hover)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    let height = this.desk.read(cx).docked_height();
                    this.held = Some(Hold::Foot(event.position, height));
                    cx.stop_propagation();
                }),
            )
    }
}

impl Held for CallStage {
    fn moved(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let (from, height) = match &mut self.held {
            Some(Hold::Foot(from, height)) => (*from, *height),
            Some(Hold::Tile(grab)) => {
                grab.follow(at);
                cx.notify();
                return;
            }
            None => return,
        };
        let tallest = self.tallest.max(px(LEAST_HEIGHT));
        let height = (height + (at.y - from.y)).clamp(px(LEAST_HEIGHT), tallest);
        self.desk
            .update(cx, |desk, cx| desk.set_docked_height(height, cx));
    }

    fn released(&mut self, _: bool, cx: &mut Context<Self>) {
        if let Some(Hold::Tile(grab)) = self.held.take() {
            let stage = self.area.get();
            match grab.dropped(stage, self.arrangement.corner, self.small_count) {
                Some(corner) => self.arrangement.corner = corner,
                None => self.arrangement.focus(grab.tile),
            }
        }
        cx.notify();
    }
}

impl Render for CallStage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let desk = self.desk.read(cx);
        let layout = desk.layout();
        let Some(call) = desk.call(cx) else {
            self.paints.clear(window);
            return div().size_full().bg(rgb(theme.pane)).into_any_element();
        };
        self.tallest = window.viewport_size().height - px(LEAST_CHAT + 100.0);
        let ended = matches!(call.phase, Phase::Ended(_));
        let main = self.home == Home::Main;
        let stage = if let Some(video) = call.video.clone().filter(|_| !ended) {
            self.pictured(&call, &video, theme, window, cx)
        } else {
            self.paints.clear(window);
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .bg(rgb(theme.pane))
                .child(self.person(&call, cx))
                .child(
                    div()
                        .pb_6()
                        .when(!ended, |row| row.child(Self::controls(&call, cx))),
                )
                .into_any_element()
        };
        div()
            .size_full()
            .relative()
            .child(follower(
                cx.weak_entity(),
                self.held.is_some(),
                Rc::clone(&self.area),
            ))
            .child(stage)
            .when(main && !ended, |stage| {
                stage.child(Self::arrangement(layout, cx))
            })
            .when(main && layout == CallLayout::Docked, |stage| {
                stage.child(Self::foot(theme, cx))
            })
            .into_any_element()
    }
}
