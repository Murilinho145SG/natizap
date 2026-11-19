//! The call shown large: over the chat in the main window, docked with the
//! chat below it or filling the chat's area, and in a window of its own. A
//! voice call shows the person in the middle, with the call's clock under
//! them in the call's own window (the main window's bar shows it there);
//! a video call shows its pictures as tiles (see `tiles`). The controls sit at the bottom; in the main
//! window, its arrangement's buttons sit at the top right, and a docked
//! stage's foot is dragged to make it taller or shorter.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

use gpui::prelude::*;
use gpui::{
    Bounds, ClickEvent, Context, Entity, EventEmitter, FontWeight, MouseButton, MouseDownEvent,
    Pixels, Point, Window, div, px, rgb, rgba,
};
use natizap_profiles::ProfilePictures;
use natizap_ui::theme::{ActiveTheme as _, DISPLAY_FONT, Theme};
use natizap_ui::{Button, ButtonSize, ButtonVariant, Icon, Menu, avatar};

use crate::bar::{CallPoppedOut, camera_button, mute_button, phase_line, screen_button};
use crate::desk::{CallDesk, CallLayout, CallShown, Phase};
use crate::keys::HANG_UP;
use crate::look::ringed;
use crate::person_menu::{peer_notes, person_menu};
use crate::pointer::{Held, follower};
use crate::tiles::{Arrangement, Grab, group_size};

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
    /// Where the menu of where the call shows opened, while it is open.
    layout_menu: Option<Point<Pixels>>,
    /// Where the menu of how the other end is heard opened, while open.
    person_menu: Option<Point<Pixels>>,
    /// The small tiles gliding into their corner: from where they were
    /// let go, and since when.
    glide: Option<(Point<Pixels>, Instant)>,
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
            layout_menu: None,
            person_menu: None,
            glide: None,
            held: None,
            area: Rc::default(),
            tallest: px(f32::MAX),
        }
    }

    /// Opens the menu of how the other end is heard where `event` was.
    fn open_person_menu(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.person_menu = Some(event.position);
        cx.stop_propagation();
        cx.notify();
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
            .child(
                div()
                    .id("call-stage-person")
                    .on_mouse_down(MouseButton::Right, cx.listener(Self::open_person_menu))
                    .child(ringed(
                        "call-stage-speaking",
                        avatar(call.peer.as_str(), &call.title, picture, px(96.0)),
                        96.0,
                        call.speaking,
                        theme,
                    )),
            )
            .child(peer_notes(call, theme))
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
        // On a dark pill, so they read over any picture.
        div()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .py_2()
            .rounded_full()
            .bg(rgba(cx.theme().scrim))
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

    /// The buttons at the stage's top right: where the call shows in the
    /// main window, its own window, and the whole screen.
    fn arrangement(&self, full: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = *cx.theme();
        let main = self.home == Home::Main;
        let full_screen = if full {
            Button::new("call-stage-full-screen")
                .icon(Icon::FullScreenExit)
                .tooltip("Leave full screen (Esc)")
        } else {
            Button::new("call-stage-full-screen")
                .icon(Icon::FullScreen)
                .tooltip("Full screen")
        };
        let full_screen = full_screen
            .variant(ButtonVariant::Ghost)
            .round()
            .on_click(cx.listener(move |this, _, window, cx| {
                if this.home == Home::Main {
                    this.desk
                        .update(cx, |desk, cx| desk.set_full_screen(!full, cx));
                } else {
                    window.toggle_fullscreen();
                }
            }));
        div()
            .absolute()
            .top_2()
            .right_2()
            .flex()
            .items_center()
            .gap_1()
            .p_1()
            .rounded_full()
            .bg(rgba(theme.scrim))
            .when(main && !full, |row| {
                row.child(
                    Button::new("call-stage-layout")
                        .icon(Icon::Layout)
                        .variant(ButtonVariant::Ghost)
                        .round()
                        .selected(self.layout_menu.is_some())
                        .tooltip("Where the call shows")
                        .on_click(cx.listener(|this, event: &ClickEvent, _, cx| {
                            this.layout_menu = Some(event.position());
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("call-stage-pop-out")
                        .icon(Icon::PopOut)
                        .variant(ButtonVariant::Ghost)
                        .round()
                        .tooltip("Open in its own window")
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(CallPoppedOut))),
                )
                .child(div().w(px(1.0)).h(px(16.0)).mx_1().bg(rgba(theme.edge)))
            })
            .child(full_screen)
    }

    /// The menu of where the call shows in the main window, once its
    /// button was clicked: every arrangement but the one in use.
    fn layout_menu(&self, layout: CallLayout, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let position = self.layout_menu?;
        let view = cx.weak_entity();
        let arrange = move |to: Option<CallLayout>| {
            let view = view.clone();
            move |_: &mut Window, cx: &mut gpui::App| {
                let _ = view.update(cx, |this, cx| {
                    this.layout_menu = None;
                    if let Some(to) = to {
                        this.desk.update(cx, |desk, cx| desk.set_layout(to, cx));
                    }
                    cx.notify();
                });
            }
        };
        let choices = [
            (CallLayout::Docked, Icon::Restore, "Over the chat"),
            (CallLayout::Filled, Icon::Maximize, "Fill the chat's area"),
            (
                CallLayout::Floating,
                Icon::Minimize,
                "Float as a small picture",
            ),
            (CallLayout::Bar, Icon::ChevronUp, "Only the bar"),
        ];
        let mut menu = Menu::new();
        for (to, icon, label) in choices {
            if to != layout {
                menu = menu.action(icon, label, arrange(Some(to)));
            }
        }
        Some(menu.on_dismiss(arrange(None)).at(position))
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
            let from = self.arrangement.corner;
            match grab.dropped(stage, from, self.small_count) {
                Some(corner) => {
                    // From where they were let go, they glide into place.
                    let group = group_size(self.small_count);
                    let left = from.origin(stage.size, group) + grab.moved.unwrap_or_default();
                    self.glide = Some((left, Instant::now()));
                    self.arrangement.corner = corner;
                }
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
        let full = desk.full_screen();
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
            .when(!ended, |stage| stage.child(self.arrangement(full, cx)))
            .children(self.layout_menu(layout, cx))
            .children(self.person_menu.map(|at| {
                person_menu(
                    cx,
                    |stage: &mut Self| &mut stage.person_menu,
                    &self.desk,
                    &call,
                    at,
                )
            }))
            .when(main && !full && layout == CallLayout::Docked, |stage| {
                stage.child(Self::foot(theme, cx))
            })
            // While tiles are dragged, the hand holds them wherever the
            // pointer goes over the stage.
            .when(
                matches!(self.held, Some(Hold::Tile(Grab { moved: Some(_), .. }))),
                |stage| stage.child(div().absolute().inset_0().cursor_grabbing()),
            )
            .into_any_element()
    }
}
