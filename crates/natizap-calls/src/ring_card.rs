//! A call ringing here, as a card floating over the main window: who
//! calls, their picture filling its top, and the answers. It starts in the
//! top right corner and goes wherever the user drags it, as on Discord,
//! staying there for the next calls.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    Bounds, Context, Div, Entity, FontWeight, MouseButton, MouseDownEvent, ObjectFit, Pixels,
    Point, SharedString, Window, div, img, linear_color_stop, linear_gradient, point, px, rgb, svg,
};
use natizap_profiles::ProfilePictures;
use natizap_protocol::Ring;
use natizap_ui::motion::rise_in;
use natizap_ui::theme::{ActiveTheme as _, DISPLAY_FONT, Theme};
use natizap_ui::{Button, ButtonSize, ButtonVariant, Icon, avatar};

use crate::desk::CallDesk;
use crate::look::ringed;
use crate::pointer::{DRAG_SLOP, Held, follower, inside};

/// The card's width.
const WIDTH: f32 = 300.0;
/// How tall the caller's picture fills the card's top.
const BACKDROP: f32 = 150.0;
/// The gap the card keeps from the window's edges where it starts.
const MARGIN: f32 = 12.0;

/// The call ringing here, as a card floating over the window. Draws
/// nothing while nothing rings.
pub struct RingCard {
    desk: Entity<CallDesk>,
    pictures: Entity<ProfilePictures>,
    /// Where the user dragged the card: its top left corner in the area
    /// it floats over. `None` until dragged.
    place: Option<Point<Pixels>>,
    /// The pointer holding the card: where it went down, where in the card
    /// it took hold, and whether it went far enough to be a drag.
    held: Option<(Point<Pixels>, Point<Pixels>, bool)>,
    /// The area the card floats over, and the card, as last painted.
    area: Rc<Cell<Bounds<Pixels>>>,
    card: Rc<Cell<Bounds<Pixels>>>,
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
        Self {
            desk,
            pictures,
            place: None,
            held: None,
            area: Rc::default(),
            card: Rc::default(),
        }
    }

    /// Where the card's top left corner sits in its area: where the user
    /// put it, else the top right corner.
    fn place(&self) -> Point<Pixels> {
        let area = self.area.get();
        let card = self.card.get().size;
        let start = point(area.size.width - px(WIDTH + MARGIN), px(MARGIN));
        inside(self.place.unwrap_or(start), card, area)
    }
}

impl Held for RingCard {
    fn moved(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some((from, grip, moved)) = self.held.as_mut() else {
            return;
        };
        let away = at - *from;
        if !*moved && f32::from(away.x).hypot(f32::from(away.y)) < DRAG_SLOP {
            return;
        }
        *moved = true;
        let area = self.area.get();
        self.place = Some(inside(at - *grip - area.origin, self.card.get().size, area));
        cx.notify();
    }

    fn released(&mut self, _: bool, cx: &mut Context<Self>) {
        self.held = None;
        cx.notify();
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
        let place = self.place();
        let dragging = self.held.is_some_and(|(_, _, moved)| moved);
        let measure = Rc::clone(&self.card);
        let card = theme
            .glass(div())
            .id("ring-card")
            .relative()
            .w(px(WIDTH))
            .overflow_hidden()
            .rounded_2xl()
            .bg(rgb(theme.raised))
            .when_else(dragging, Styled::cursor_grabbing, Styled::cursor_grab)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|card, event: &MouseDownEvent, _, cx| {
                    let grip = event.position - card.area.get().origin - card.place();
                    card.held = Some((event.position, grip, false));
                    cx.notify();
                }),
            )
            .child(
                gpui::canvas(move |bounds, _, _| measure.set(bounds), |_, (), _, _| {})
                    .absolute()
                    .size_full(),
            )
            .children(picture.clone().map(|picture| backdrop(picture, theme)))
            .child(Self::body(&ring, title, picture, in_call, theme, cx));
        div()
            .absolute()
            .inset_0()
            .child(follower(
                cx.weak_entity(),
                self.held.is_some(),
                Rc::clone(&self.area),
            ))
            .child(
                div()
                    .absolute()
                    .left(place.x)
                    .top(place.y)
                    .occlude()
                    .child(rise_in(
                        SharedString::from(format!("ring-card-{}", ring.id)),
                        card,
                        -12.0,
                    )),
            )
            .into_any_element()
    }
}

impl RingCard {
    /// The caller, what rings, and the answers.
    fn body(
        ring: &Ring,
        title: SharedString,
        picture: Option<PathBuf>,
        in_call: bool,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .relative()
            .px_5()
            .pt_6()
            .pb_5()
            .flex()
            .flex_col()
            .items_center()
            .child(ringed(
                SharedString::from(format!("ring-pulse-{}", ring.id)),
                avatar(ring.caller.as_str(), &title, picture, px(84.0)),
                84.0,
                true,
                theme,
            ))
            .child(
                div()
                    .pt_4()
                    .max_w_full()
                    .truncate()
                    .font_family(DISPLAY_FONT)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(20.0))
                    .child(title),
            )
            .child(kind_line(ring.video, in_call, theme))
            .child(
                div()
                    .pt_6()
                    .w_full()
                    .flex()
                    .justify_around()
                    .child(answer(
                        (
                            "ring-decline",
                            Icon::CallEnd,
                            ButtonVariant::Danger,
                            "Decline",
                        ),
                        (false, theme),
                        cx.listener(|card, _, _, cx| {
                            card.desk.update(cx, CallDesk::decline);
                        }),
                    ))
                    .child(answer(
                        (
                            "ring-answer",
                            if ring.video { Icon::Video } else { Icon::Call },
                            ButtonVariant::Primary,
                            "Answer",
                        ),
                        (true, theme),
                        cx.listener(|card, _, _, cx| {
                            card.desk.update(cx, CallDesk::answer);
                        }),
                    )),
            )
    }
}

/// The caller's picture filling the card's top, dimmed and fading into
/// the card, so the card is theirs at a glance and its words still read.
fn backdrop(picture: PathBuf, theme: Theme) -> Div {
    let card = rgb(theme.raised);
    div()
        .absolute()
        .top_0()
        .left_0()
        .w(px(WIDTH))
        .h(px(BACKDROP))
        .child(
            img(picture)
                .size_full()
                .object_fit(ObjectFit::Cover)
                .opacity(0.35),
        )
        .child(div().absolute().inset_0().bg(linear_gradient(
            180.0,
            linear_color_stop(card.opacity(0.2), 0.0),
            linear_color_stop(card, 1.0),
        )))
}

/// What rings, under the caller's name.
fn kind_line(video: bool, in_call: bool, theme: Theme) -> Div {
    div()
        .pt_1()
        .flex()
        .items_center()
        .gap_1p5()
        .text_size(px(12.5))
        .text_color(rgb(theme.muted))
        .child(
            svg()
                .path(if video { Icon::Video } else { Icon::Call }.path())
                .size(px(13.0))
                .text_color(rgb(theme.accent)),
        )
        .child(match (video, in_call) {
            (true, false) => "Incoming video call",
            (false, false) => "Incoming voice call",
            // Answering ends the call under way.
            (_, true) => "Calling you · answering ends your call",
        })
}

/// A round answer to the ringing call, named under it; the one taking
/// the call pulses, as an invitation.
fn answer(
    (id, icon, variant, label): (&'static str, Icon, ButtonVariant, &'static str),
    (pulsing, theme): (bool, Theme),
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> Div {
    let button = div().child(
        Button::new(id)
            .icon(icon)
            .variant(variant)
            .size(ButtonSize::Large)
            .round()
            .tooltip(label)
            .on_click(on_click),
    );
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap_2()
        .child(if pulsing {
            ringed(SharedString::from(id), button, 44.0, true, theme).into_any_element()
        } else {
            button.into_any_element()
        })
        .child(
            div()
                .text_size(px(12.0))
                .text_color(rgb(theme.muted))
                .child(label),
        )
}
