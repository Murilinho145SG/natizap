//! The post itself between the arrows that step through the statuses: a
//! press on it holds it, a tap steps, and a click beside it closes.

use std::time::Instant;

use gpui::prelude::*;
use gpui::{
    AnyElement, Context, FontWeight, ImageSource, MouseButton, MouseDownEvent, MouseUpEvent,
    ObjectFit, div, img, px, rgb, rgba, svg,
};
use natizap_protocol::ContentKind;
use natizap_ui::theme::{MUTED, STATUS_BACKGROUNDS};
use natizap_ui::{Button, ButtonVariant, Icon};

use super::{StatusClosed, StatusViewer, TAP};
use crate::post::Post;

impl StatusViewer {
    /// The post with an arrow on each side; a click on neither closes.
    pub(super) fn stage(&self, post: &Post, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .id("status-stage")
            .flex_1()
            .min_h_0()
            .p_4()
            .flex()
            .items_center()
            .gap_4()
            .on_click(cx.listener(|_, _, _, cx| cx.emit(StatusClosed)))
            .child(arrow(
                "status-back",
                Icon::ChevronLeft,
                cx.listener(|this, _, _, cx| this.back(cx)),
            ))
            .child(self.post_column(post, cx))
            .child(arrow(
                "status-forward",
                Icon::ChevronRight,
                cx.listener(|this, _, _, cx| this.forward(cx)),
            ))
    }

    /// The post and its caption, sized to the post so clicks beside it
    /// reach the stage; a press holds it, a tap on its left third steps
    /// back and anywhere else on.
    fn post_column(&self, post: &Post, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let content = self.content(post);
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("status-post")
                    .max_w_full()
                    .min_h_0()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when_some(aspect(post), |frame, ratio| {
                        frame.h_full().aspect_ratio(ratio)
                    })
                    .when(post.kind == ContentKind::Text, |frame| {
                        frame.w_full().max_w(px(720.0))
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.pressed = Some(Instant::now());
                            this.hold(true);
                            cx.notify();
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseUpEvent, window, cx| {
                            cx.stop_propagation();
                            this.hold(false);
                            let tapped = this.pressed.take().is_some_and(|at| at.elapsed() < TAP);
                            if tapped {
                                let third = window.viewport_size().width / 3.0;
                                if event.position.x < third {
                                    this.back(cx);
                                } else {
                                    this.forward(cx);
                                }
                            }
                            cx.notify();
                        }),
                    )
                    .child(content),
            )
            .children(
                post.text
                    .clone()
                    .filter(|_| post.kind != ContentKind::Text)
                    .map(|caption| {
                        div()
                            .pt_3()
                            .max_w(px(640.0))
                            .text_center()
                            .text_size(px(15.0))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(caption)
                    }),
            )
    }

    fn content(&self, post: &Post) -> AnyElement {
        match (post.kind, &self.player, &post.file) {
            // The player's own controls take its clicks.
            (ContentKind::Video, Some((id, player)), _) if *id == post.id => div()
                .size_full()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(player.clone())
                .into_any_element(),
            (ContentKind::Text, ..) => text_post(post).into_any_element(),
            (_, _, file) => {
                let source: Option<ImageSource> = match (post.kind, file) {
                    (ContentKind::Photo, Some(file)) => Some(file.clone().into()),
                    _ => post
                        .media
                        .as_ref()
                        .and_then(|media| media.thumbnail.clone())
                        .map(|bytes| {
                            std::sync::Arc::new(gpui::Image::from_bytes(
                                gpui::ImageFormat::Jpeg,
                                bytes,
                            ))
                            .into()
                        }),
                };
                match source {
                    Some(source) => img(source)
                        .size_full()
                        .object_fit(ObjectFit::Contain)
                        .into_any_element(),
                    None => svg()
                        .path(Icon::Camera.path())
                        .size(px(48.0))
                        .text_color(rgb(MUTED))
                        .into_any_element(),
                }
            }
        }
    }
}

/// A round arrow beside the post; its clicks stay off the stage behind.
fn arrow(
    id: &'static str,
    icon: Icon,
    on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .flex_none()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            Button::new(id)
                .icon(icon)
                .variant(ButtonVariant::Secondary)
                .round()
                .on_click(on_click),
        )
}

/// A photo or video's width over its height, when it says.
#[expect(
    clippy::cast_precision_loss,
    reason = "picture sides are far below f32's exact range"
)]
fn aspect(post: &Post) -> Option<f32> {
    if !matches!(post.kind, ContentKind::Photo | ContentKind::Video) {
        return None;
    }
    let media = post.media.as_ref()?;
    let (width, height) = (media.width?, media.height?);
    (width > 0 && height > 0).then(|| width as f32 / height as f32)
}

/// A text post: its words large on its background colour.
fn text_post(post: &Post) -> impl IntoElement {
    let background = argb_to_rgba(post.background.unwrap_or(STATUS_BACKGROUNDS[7]));
    div()
        .size_full()
        .max_w(px(720.0))
        .rounded_lg()
        .bg(rgba(background))
        .p_8()
        .flex()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .child(
            // Its own width, so long words and links wrap instead of
            // running past the colour.
            div()
                .w_full()
                .text_center()
                .text_size(px(text_size(post.text.as_deref().unwrap_or_default())))
                .font_weight(FontWeight::MEDIUM)
                .child(post.text.clone().unwrap_or_default()),
        )
}

/// Smaller type for longer text, as the phone sets it, so it fits.
fn text_size(text: &str) -> f32 {
    match text.chars().count() {
        0..=60 => 30.0,
        61..=160 => 24.0,
        161..=320 => 19.0,
        _ => 16.0,
    }
}

/// WhatsApp's 0xAARRGGBB as GPUI's 0xRRGGBBAA.
fn argb_to_rgba(argb: u32) -> u32 {
    argb.rotate_left(8)
}
