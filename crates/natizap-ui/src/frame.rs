use gpui::prelude::*;
use gpui::{
    AnyElement, Bounds, CursorStyle, Decorations, Hitbox, HitboxBehavior, Hsla, MouseButton,
    Pixels, Point, ResizeEdge, Size, Tiling, Window, canvas, div, point, px, rgb, svg,
    transparent_black,
};

use crate::Icon;
use crate::theme::{ALERT, DEEP, LINE, MUTED, RAISED, TEXT};

/// Width of the invisible band around the window that resizes it, also used
/// for the shadow.
const EDGE: Pixels = px(10.0);
const CORNER_RADIUS: Pixels = px(10.0);
const TITLE_BAR_HEIGHT: Pixels = px(40.0);

/// Frames the window's content under natizap's title bar.
///
/// When the compositor leaves decorations to the app (GNOME on Wayland does),
/// it also draws the rounded corners, shadow, resize edges and window
/// buttons; otherwise the compositor's own are used. `title` goes on the left
/// of the title bar, which also moves the window when dragged.
pub fn window_frame(
    window: &mut Window,
    title: impl IntoElement,
    content: impl IntoElement,
) -> impl IntoElement {
    let decorations = window.window_decorations();
    let body = div()
        .size_full()
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(rgb(DEEP))
        .cursor(CursorStyle::Arrow)
        .on_mouse_move(|_, _, cx| cx.stop_propagation())
        .child(title_bar(window, title, decorations))
        .child(div().flex_1().min_h_0().child(content));

    let Decorations::Client { tiling } = decorations else {
        window.set_client_inset(px(0.0));
        return div().size_full().child(body).into_any_element();
    };

    window.set_client_inset(EDGE);
    let body = body
        .border_color(rgb(LINE))
        .when(!tiling.top, Styled::border_t_1)
        .when(!tiling.bottom, Styled::border_b_1)
        .when(!tiling.left, Styled::border_l_1)
        .when(!tiling.right, Styled::border_r_1)
        .when(!(tiling.top || tiling.left), |body| {
            body.rounded_tl(CORNER_RADIUS)
        })
        .when(!(tiling.top || tiling.right), |body| {
            body.rounded_tr(CORNER_RADIUS)
        })
        .when(!(tiling.bottom || tiling.left), |body| {
            body.rounded_bl(CORNER_RADIUS)
        })
        .when(!(tiling.bottom || tiling.right), |body| {
            body.rounded_br(CORNER_RADIUS)
        })
        .when(!tiling.is_tiled(), |body| {
            body.shadow(vec![
                gpui::BoxShadow::new(px(0.0), px(2.0), Hsla::black().opacity(0.45))
                    .blur_radius(EDGE / 2.0),
            ])
        });

    div()
        .id("window-edges")
        .size_full()
        .bg(transparent_black())
        .when(!tiling.top, |edges| edges.pt(EDGE))
        .when(!tiling.bottom, |edges| edges.pb(EDGE))
        .when(!tiling.left, |edges| edges.pl(EDGE))
        .when(!tiling.right, |edges| edges.pr(EDGE))
        .child(
            canvas(resize_hitbox, move |_, hitbox, window, _| {
                set_resize_cursor(&hitbox, tiling, window);
            })
            .size_full()
            .absolute(),
        )
        .on_mouse_move(|_, window, _| window.refresh())
        .on_mouse_down(MouseButton::Left, move |event, window, _| {
            if let Some(edge) = resize_edge(event.position, window.viewport_size(), tiling) {
                window.start_window_resize(edge);
            }
        })
        .child(body)
        .into_any_element()
}

fn title_bar(window: &Window, title: impl IntoElement, decorations: Decorations) -> AnyElement {
    let maximized = window.is_maximized();
    div()
        .id("title-bar")
        .h(TITLE_BAR_HEIGHT)
        .flex_none()
        .flex()
        .items_center()
        .pl_4()
        .pr_1()
        .border_b_1()
        .border_color(rgb(LINE))
        .on_mouse_down(MouseButton::Left, |event, window, _| {
            if event.click_count == 2 {
                window.zoom_window();
            } else {
                window.start_window_move();
            }
        })
        .on_mouse_down(MouseButton::Right, |event, window, _| {
            window.show_window_menu(event.position);
        })
        .child(div().flex_1().min_w_0().child(title))
        .when(matches!(decorations, Decorations::Client { .. }), |bar| {
            bar.child(
                div()
                    .flex()
                    .gap_0p5()
                    .child(window_button(
                        "minimize",
                        Icon::Minimize,
                        RAISED,
                        |window| {
                            window.minimize_window();
                        },
                    ))
                    .child(window_button(
                        "maximize",
                        if maximized {
                            Icon::Restore
                        } else {
                            Icon::Maximize
                        },
                        RAISED,
                        |window| window.zoom_window(),
                    ))
                    .child(window_button(
                        "close",
                        Icon::Close,
                        ALERT,
                        Window::remove_window,
                    )),
            )
        })
        .into_any_element()
}

fn window_button(
    id: &'static str,
    icon: Icon,
    hover: u32,
    action: impl Fn(&mut Window) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .w(px(36.0))
        .h(px(28.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .text_color(rgb(MUTED))
        .hover(move |style| style.bg(rgb(hover)).text_color(rgb(TEXT)))
        // Keep the title bar from starting a window move under the button.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, _| action(window))
        .child(
            svg()
                .path(icon.path())
                .size(px(16.0))
                .text_color(rgb(MUTED)),
        )
}

fn resize_hitbox(_: Bounds<Pixels>, window: &mut Window, _: &mut gpui::App) -> Hitbox {
    let size = window.viewport_size();
    window.insert_hitbox(
        Bounds::new(point(px(0.0), px(0.0)), size),
        HitboxBehavior::Normal,
    )
}

fn set_resize_cursor(hitbox: &Hitbox, tiling: Tiling, window: &mut Window) {
    let Some(edge) = resize_edge(window.mouse_position(), window.viewport_size(), tiling) else {
        return;
    };
    let cursor = match edge {
        ResizeEdge::Top | ResizeEdge::Bottom => CursorStyle::ResizeUpDown,
        ResizeEdge::Left | ResizeEdge::Right => CursorStyle::ResizeLeftRight,
        ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorStyle::ResizeUpLeftDownRight,
        ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorStyle::ResizeUpRightDownLeft,
    };
    window.set_cursor_style(cursor, hitbox);
}

/// Which window edge or corner `position` is on, if any. `size` must be the
/// window's current size: `window_bounds()` reports the restored size of a
/// maximized window, which once turned most of a maximized window into a
/// resize handle that swallowed clicks. Edges against the screen (tiled) do
/// not resize.
fn resize_edge(position: Point<Pixels>, size: Size<Pixels>, tiling: Tiling) -> Option<ResizeEdge> {
    let top = !tiling.top && position.y < EDGE;
    let bottom = !tiling.bottom && position.y > size.height - EDGE;
    let left = !tiling.left && position.x < EDGE;
    let right = !tiling.right && position.x > size.width - EDGE;
    Some(match (top, bottom, left, right) {
        (true, _, true, _) => ResizeEdge::TopLeft,
        (true, _, _, true) => ResizeEdge::TopRight,
        (_, true, true, _) => ResizeEdge::BottomLeft,
        (_, true, _, true) => ResizeEdge::BottomRight,
        (true, ..) => ResizeEdge::Top,
        (_, true, ..) => ResizeEdge::Bottom,
        (_, _, true, _) => ResizeEdge::Left,
        (_, _, _, true) => ResizeEdge::Right,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Size<Pixels> = Size {
        width: px(1920.0),
        height: px(1048.0),
    };

    #[test]
    fn only_the_outer_band_of_a_floating_window_resizes() {
        let floating = Tiling::default();
        assert_eq!(
            resize_edge(point(px(5.0), px(500.0)), WINDOW, floating),
            Some(ResizeEdge::Left)
        );
        assert_eq!(
            resize_edge(point(px(1915.0), px(1045.0)), WINDOW, floating),
            Some(ResizeEdge::BottomRight)
        );
        assert_eq!(
            resize_edge(point(px(430.0), px(1016.0)), WINDOW, floating),
            None
        );
    }

    #[test]
    fn a_maximized_window_has_no_resize_edges() {
        let maximized = Tiling {
            top: true,
            left: true,
            right: true,
            bottom: true,
        };
        assert_eq!(
            resize_edge(point(px(5.0), px(1045.0)), WINDOW, maximized),
            None
        );
    }
}
