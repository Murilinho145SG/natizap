//! How a quoted message looks at the top of a reply, and the name-and-number
//! line group messages and quotes share.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{Div, FontWeight, Image, ObjectFit, SharedString, div, img, px, rgb, svg};
use natizap_protocol::ContentKind;
use natizap_ui::Icon;
use natizap_ui::theme::{DEEP, MUTED};

use crate::items::QuotePreview;

/// A quoted message: who wrote it over one line of what they said, marked
/// by a bar in their color.
pub(crate) fn quoted(quote: &QuotePreview) -> Div {
    quote_block(
        quote.author.clone(),
        quote.phone.clone(),
        quote.color,
        quote.text.clone(),
        quote.kind,
        quote.thumbnail.clone(),
    )
}

/// A quoted message: a bar in the author's color, the author, an icon for
/// what it carries and a line of it, and a photo or video's preview on the
/// right, as in WhatsApp.
pub(crate) fn quote_block(
    author: SharedString,
    phone: Option<SharedString>,
    color: u32,
    text: SharedString,
    kind: ContentKind,
    thumbnail: Option<Arc<Image>>,
) -> Div {
    let icon = match kind {
        ContentKind::Photo | ContentKind::Sticker => Some(Icon::Camera),
        ContentKind::Video => Some(Icon::Video),
        ContentKind::VoiceMessage => Some(Icon::Mic),
        ContentKind::Audio => Some(Icon::Waveform),
        ContentKind::Document => Some(Icon::File),
        ContentKind::Text | ContentKind::Location | ContentKind::Contact | ContentKind::Poll => {
            None
        }
    };
    div()
        .flex()
        .rounded(px(8.0))
        .overflow_hidden()
        .bg(rgb(DEEP))
        .child(div().w(px(3.0)).flex_none().bg(rgb(color)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .px_2()
                .py_1()
                .child(name_and_phone(author, color, phone))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_size(px(13.0))
                        .text_color(rgb(MUTED))
                        .children(icon.map(|icon| {
                            svg()
                                .path(icon.path())
                                .size(px(14.0))
                                .flex_none()
                                .text_color(rgb(MUTED))
                        }))
                        .child(div().min_w_0().truncate().child(text)),
                ),
        )
        .children(thumbnail.map(|thumbnail| {
            img(thumbnail)
                .w(px(48.0))
                .flex_none()
                .self_stretch()
                .rounded_r(px(8.0))
                .object_fit(ObjectFit::Cover)
        }))
}

/// A name in its color, and a number on the right beside it when there
/// is one.
pub(crate) fn name_and_phone(name: SharedString, color: u32, phone: Option<SharedString>) -> Div {
    div()
        .flex()
        .items_baseline()
        .justify_between()
        .gap_4()
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(color))
                .child(name),
        )
        .children(phone.map(|phone| {
            div()
                .flex_none()
                .text_size(px(12.0))
                .text_color(rgb(MUTED))
                .child(phone)
        }))
}
