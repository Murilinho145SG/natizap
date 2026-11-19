//! A colour picker: a square of saturation and brightness under a hue
//! slider, and an opacity slider for see-through colours. Dragging on any
//! of them picks; every change is told as [`ColorPicked`].

use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    Bounds, Context, EventEmitter, Hsla, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels,
    Point, Window, canvas, div, linear_color_stop, linear_gradient, px, relative, rgb, rgba,
};

use crate::theme::ActiveTheme as _;

/// The colour picked: `0xRRGGBB`, or `0xRRGGBBAA` for a see-through one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorPicked(pub u32);

/// The user pressed outside the picker: it should close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorPickerDismissed;

const WIDTH: f32 = 232.0;
const SQUARE_HEIGHT: f32 = 140.0;
const BAR_HEIGHT: f32 = 12.0;
const KNOB: f32 = 14.0;

type Area = Rc<Cell<Option<Bounds<Pixels>>>>;

/// Which part of the picker a drag moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Square,
    Hue,
    Alpha,
}

/// Picks a colour by hue, saturation and brightness, and opacity when
/// asked to.
pub struct ColorPicker {
    /// 0 to 1 around the wheel.
    hue: f32,
    saturation: f32,
    value: f32,
    /// The opacity, for a see-through colour.
    alpha: Option<f32>,
    square: Area,
    hue_bar: Area,
    alpha_bar: Area,
}

impl std::fmt::Debug for ColorPicker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ColorPicker")
            .field("color", &self.color())
            .finish_non_exhaustive()
    }
}

impl EventEmitter<ColorPicked> for ColorPicker {}
impl EventEmitter<ColorPickerDismissed> for ColorPicker {}

impl ColorPicker {
    /// Starts on `color`: `0xRRGGBBAA` when `see_through`, else `0xRRGGBB`.
    #[must_use]
    pub fn new(color: u32, see_through: bool) -> Self {
        let (rgb, alpha) = if see_through {
            (color >> 8, Some(f32::from((color & 0xff) as u8) / 255.0))
        } else {
            (color, None)
        };
        let (hue, saturation, value) = to_hsv(rgb);
        Self {
            hue,
            saturation,
            value,
            alpha,
            square: Area::default(),
            hue_bar: Area::default(),
            alpha_bar: Area::default(),
        }
    }

    /// The colour picked, as [`ColorPicked`] tells it.
    #[must_use]
    pub fn color(&self) -> u32 {
        let rgb = to_rgb(self.hue, self.saturation, self.value);
        match self.alpha {
            Some(alpha) => (rgb << 8) | channel(alpha),
            None => rgb,
        }
    }

    fn pick(&mut self, part: Part, at: Point<Pixels>, cx: &mut Context<Self>) {
        let area = match part {
            Part::Square => &self.square,
            Part::Hue => &self.hue_bar,
            Part::Alpha => &self.alpha_bar,
        };
        let Some(bounds) = area.get() else {
            return;
        };
        let x = ((at.x - bounds.origin.x) / bounds.size.width).clamp(0.0, 1.0);
        let y = ((at.y - bounds.origin.y) / bounds.size.height).clamp(0.0, 1.0);
        match part {
            Part::Square => {
                self.saturation = x;
                self.value = 1.0 - y;
            }
            Part::Hue => self.hue = x,
            Part::Alpha => self.alpha = Some(x),
        }
        cx.emit(ColorPicked(self.color()));
        cx.notify();
    }

    /// A part that picks where it is pressed and dragged.
    fn draggable(part: Part, area: &Area, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let measure = Rc::clone(area);
        div()
            .id(match part {
                Part::Square => "picker-square",
                Part::Hue => "picker-hue",
                Part::Alpha => "picker-alpha",
            })
            .relative()
            .w_full()
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.pick(part, event.position, cx);
                }),
            )
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    this.pick(part, event.position, cx);
                }
            }))
            .child(
                canvas(
                    move |bounds, _, _| measure.set(Some(bounds)),
                    |_, (), _, _| {},
                )
                .absolute()
                .size_full(),
            )
    }
}

/// The ring that marks where a part stands.
fn knob(left: f32, top: f32, fill: Hsla, edge: u32) -> gpui::Div {
    div()
        .absolute()
        .left(relative(left))
        .top(relative(top))
        .ml(px(-KNOB / 2.0))
        .mt(px(-KNOB / 2.0))
        .size(px(KNOB))
        .rounded_full()
        .border_2()
        .border_color(rgb(edge))
        .bg(fill)
        .shadow_sm()
}

impl Render for ColorPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let pure: Hsla = rgb(to_rgb(self.hue, 1.0, 1.0)).into();
        let picked: Hsla = rgb(to_rgb(self.hue, self.saturation, self.value)).into();
        let square = Self::draggable(Part::Square, &self.square, cx)
            .h(px(SQUARE_HEIGHT))
            .rounded_lg()
            .overflow_hidden()
            .bg(pure)
            // White fades out to the right, black in towards the bottom.
            .child(div().absolute().size_full().bg(linear_gradient(
                90.0,
                linear_color_stop(gpui::white(), 0.0),
                linear_color_stop(gpui::white().opacity(0.0), 1.0),
            )))
            .child(div().absolute().size_full().bg(linear_gradient(
                180.0,
                linear_color_stop(gpui::black().opacity(0.0), 0.0),
                linear_color_stop(gpui::black(), 1.0),
            )))
            .child(knob(self.saturation, 1.0 - self.value, picked, 0xff_ff_ff));
        // The wheel's six turns, each a gradient to the next.
        let turns = (0..6u8).map(|turn| {
            let from = rgb(to_rgb(f32::from(turn) / 6.0, 1.0, 1.0));
            let to = rgb(to_rgb(f32::from(turn + 1) / 6.0, 1.0, 1.0));
            div().flex_1().h_full().bg(linear_gradient(
                90.0,
                linear_color_stop(from, 0.0),
                linear_color_stop(to, 1.0),
            ))
        });
        let hue = Self::draggable(Part::Hue, &self.hue_bar, cx)
            .h(px(BAR_HEIGHT))
            .child(
                div()
                    .size_full()
                    .rounded_full()
                    .overflow_hidden()
                    .flex()
                    .children(turns),
            )
            .child(knob(self.hue, 0.5, pure, theme.text));
        let alpha = self.alpha.map(|alpha| {
            Self::draggable(Part::Alpha, &self.alpha_bar, cx)
                .h(px(BAR_HEIGHT))
                .child(
                    div()
                        .size_full()
                        .rounded_full()
                        .border_1()
                        .border_color(rgb(theme.line))
                        .bg(linear_gradient(
                            90.0,
                            linear_color_stop(picked.opacity(0.0), 0.0),
                            linear_color_stop(picked, 1.0),
                        )),
                )
                .child(knob(alpha, 0.5, picked.opacity(alpha), theme.text))
        });
        div()
            .w(px(WIDTH))
            .p_3()
            .flex()
            .flex_col()
            .gap_3()
            .rounded_xl()
            .bg(rgb(theme.raised))
            .map(|surface| theme.glass(surface))
            .occlude()
            .child(square)
            .child(hue)
            .children(alpha)
            .on_mouse_down_out(cx.listener(|_, _, _, cx| cx.emit(ColorPickerDismissed)))
            .child(self.picked(theme))
    }
}

impl ColorPicker {
    /// The colour picked, ringed so a dark one still shows, and its hex.
    fn picked(&self, theme: crate::theme::Theme) -> gpui::Div {
        let (fill, hex) = match self.alpha {
            Some(_) => (self.color(), format!("#{:08x}", self.color())),
            None => ((self.color() << 8) | 0xff, format!("#{:06x}", self.color())),
        };
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .size(px(18.0))
                    .rounded_full()
                    .border_1()
                    .border_color(rgb(theme.muted))
                    .bg(rgba(fill)),
            )
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(rgb(theme.muted))
                    .child(hex),
            )
    }
}

/// `0..=1` as a byte.
fn channel(fraction: f32) -> u32 {
    // In range by the clamp; rounding is the point.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=255"
    )]
    let byte = (fraction.clamp(0.0, 1.0) * 255.0).round() as u32;
    byte
}

/// `0xRRGGBB` as hue (0 to 1 around the wheel), saturation and value.
fn to_hsv(color: u32) -> (f32, f32, f32) {
    // Which channel is largest is told on the bytes, exactly.
    let byte = |shift: u32| ((color >> shift) & 0xff) as u8;
    let (red, green, blue) = (byte(16), byte(8), byte(0));
    let (high, low) = (red.max(green).max(blue), red.min(green).min(blue));
    let part = |value: u8| f32::from(value) / 255.0;
    let delta = part(high) - part(low);
    let hue = if high == low {
        0.0
    } else if high == red {
        ((part(green) - part(blue)) / delta).rem_euclid(6.0) / 6.0
    } else if high == green {
        ((part(blue) - part(red)) / delta + 2.0) / 6.0
    } else {
        ((part(red) - part(green)) / delta + 4.0) / 6.0
    };
    let saturation = if high == 0 { 0.0 } else { delta / part(high) };
    (hue, saturation, part(high))
}

/// Hue (0 to 1 around the wheel), saturation and value as `0xRRGGBB`.
fn to_rgb(hue: f32, saturation: f32, value: f32) -> u32 {
    let sector = (hue.rem_euclid(1.0) * 6.0).min(5.999_999);
    let chroma = value * saturation;
    let middle = chroma * (1.0 - ((sector % 2.0) - 1.0).abs());
    let floor = value - chroma;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "0..6, floored"
    )]
    let (red, green, blue) = match sector as u8 {
        0 => (chroma, middle, 0.0),
        1 => (middle, chroma, 0.0),
        2 => (0.0, chroma, middle),
        3 => (0.0, middle, chroma),
        4 => (middle, 0.0, chroma),
        _ => (chroma, 0.0, middle),
    };
    (channel(red + floor) << 16) | (channel(green + floor) << 8) | channel(blue + floor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_survive_the_round_trip() {
        for color in [
            0xff_7a_1a, 0x0f_11_14, 0xff_ff_ff, 0x00_00_00, 0x4c_8d_ff, 0x5f_cf_9f,
        ] {
            let (h, s, v) = to_hsv(color);
            assert_eq!(to_rgb(h, s, v), color, "{color:06x}");
        }
    }

    #[test]
    fn keeps_opacity_for_see_through_colours() {
        assert_eq!(ColorPicker::new(0xff_ff_ff_0d, true).color(), 0xff_ff_ff_0d);
        assert_eq!(ColorPicker::new(0x12_34_56, false).color(), 0x12_34_56);
    }
}
