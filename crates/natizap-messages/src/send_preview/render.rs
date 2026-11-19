//! How the files about to be sent are drawn: the selected one large, its
//! caption and the send button, and every file in a strip below.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    Context, Div, FontWeight, ImageSource, ObjectFit, SharedString, Stateful, Window, div, img, px,
    rgb, svg,
};
use natizap_protocol::ContentKind;
use natizap_ui::theme::{ActiveTheme as _, Theme};
use natizap_ui::{Button, ButtonSize, ButtonVariant, Icon};

use super::{MOST_FILES, PreviewClosed, SendPreview, Staged};
use crate::items::file_size;
use crate::text_pill::text_pill;

/// Side of a file's thumbnail in the strip.
const THUMB: f32 = 56.0;

impl Render for SendPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let Some(file) = self.files.get(self.selected).cloned() else {
            return div();
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme.pane))
            .child(top_bar(&file, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .p_6()
                    .child(big_preview(&file, theme)),
            )
            .child(self.caption_bar(window, cx))
            .child(self.strip(cx))
    }
}

impl SendPreview {
    /// The caption field and the send button.
    fn caption_bar(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = *cx.theme();
        let count = self.files.len();
        div()
            .flex()
            .items_end()
            .justify_center()
            .gap_3()
            .px_6()
            .pb_3()
            .child(
                text_pill("caption-scrollbar", &self.caption, window, cx)
                    .flex_1()
                    .max_w(px(640.0)),
            )
            .child(
                div()
                    .relative()
                    .child(
                        Button::new("send-files")
                            .icon(Icon::Send)
                            .size(ButtonSize::Large)
                            .round()
                            .tooltip(if count == 1 {
                                "Send".to_owned()
                            } else {
                                format!("Send {count} files")
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.send(cx))),
                    )
                    .when(count > 1, |button| {
                        button.child(
                            div()
                                .absolute()
                                .top(px(-4.0))
                                .right(px(-4.0))
                                .min_w(px(18.0))
                                .h(px(18.0))
                                .px_1()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .bg(rgb(theme.panel))
                                .border_1()
                                .border_color(rgb(theme.accent))
                                .text_size(px(11.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(theme.text))
                                .child(count.to_string()),
                        )
                    }),
            )
    }

    /// Every file's thumbnail, the selected one marked, and a tile to add
    /// more.
    fn strip(&self, cx: &mut Context<Self>) -> Div {
        let theme = *cx.theme();
        div()
            .flex()
            .justify_center()
            .flex_wrap()
            .gap_2()
            .px_6()
            .pt_1()
            .pb_4()
            .children(
                self.files
                    .iter()
                    .enumerate()
                    .map(|(index, file)| thumbnail(file, index, index == self.selected, cx)),
            )
            .when(self.files.len() < MOST_FILES, |strip| {
                strip.child(
                    div()
                        .id("add-files")
                        .size(px(THUMB))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(theme.line))
                        .cursor_pointer()
                        .hover(|tile| tile.bg(rgb(theme.raised)))
                        .on_click(cx.listener(|_, _, _, cx| Self::add_more(cx)))
                        .child(
                            svg()
                                .path(Icon::Plus.path())
                                .size(px(20.0))
                                .text_color(rgb(theme.muted)),
                        ),
                )
            })
    }
}

/// Close, and the selected file's name and size.
fn top_bar(file: &Staged, cx: &mut Context<SendPreview>) -> Div {
    let theme = *cx.theme();
    div()
        .flex()
        .items_center()
        .gap_3()
        .px_4()
        .py_3()
        .child(
            Button::new("close-preview")
                .icon(Icon::Close)
                .variant(ButtonVariant::Ghost)
                .round()
                .tooltip("Cancel")
                .on_click(cx.listener(|_, _, _, cx| cx.emit(PreviewClosed))),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .truncate()
                        .font_weight(FontWeight::MEDIUM)
                        .child(file.name.clone()),
                )
                .children(file.size.map(|size| {
                    div()
                        .text_size(px(12.5))
                        .text_color(rgb(theme.muted))
                        .child(file_size(size))
                })),
        )
}

/// The selected file, as large as fits: photos and video stills as
/// pictures, anything else as a card with its kind.
fn big_preview(file: &Staged, theme: Theme) -> Div {
    let picture: Option<ImageSource> = match (file.kind, &file.still) {
        (ContentKind::Photo, _) => Some(file.path.clone().into()),
        (ContentKind::Video, Some(still)) => Some(Arc::clone(still).into()),
        _ => None,
    };
    if let Some(picture) = picture {
        return div()
            .size_full()
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .child(
                img(picture)
                    .size_full()
                    .rounded(px(8.0))
                    .object_fit(ObjectFit::Contain),
            )
            .when(file.kind == ContentKind::Video, |preview| {
                preview.child(
                    div()
                        .absolute()
                        .size(px(56.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(rgb(theme.panel))
                        .opacity(0.85)
                        .child(
                            svg()
                                .path(Icon::Play.path())
                                .size(px(22.0))
                                .text_color(rgb(theme.text)),
                        ),
                )
            });
    }
    div()
        .w(px(320.0))
        .flex()
        .flex_col()
        .items_center()
        .gap_3()
        .p_8()
        .rounded_xl()
        .bg(rgb(theme.panel))
        .map(|surface| theme.glass(surface))
        .child(
            svg()
                .path(kind_icon(file.kind).path())
                .size(px(56.0))
                .text_color(rgb(theme.muted)),
        )
        .child(
            div()
                .w_full()
                .text_center()
                .truncate()
                .font_weight(FontWeight::MEDIUM)
                .child(file.name.clone()),
        )
        .child(
            div()
                .text_size(px(13.0))
                .text_color(rgb(theme.muted))
                .child(kind_line(file)),
        )
}

/// A file in the strip; clicking selects it, and its corner removes it.
fn thumbnail(
    file: &Staged,
    index: usize,
    selected: bool,
    cx: &mut Context<SendPreview>,
) -> Stateful<Div> {
    let theme = *cx.theme();
    let group = SharedString::from(format!("staged-{index}"));
    let picture: Option<ImageSource> = match (file.kind, &file.still) {
        (ContentKind::Photo, _) => Some(file.path.clone().into()),
        (ContentKind::Video, Some(still)) => Some(Arc::clone(still).into()),
        _ => None,
    };
    div()
        .id(("staged", index))
        .group(group.clone())
        .relative()
        .size(px(THUMB))
        .flex()
        .items_center()
        .justify_center()
        .rounded_lg()
        .bg(rgb(theme.panel))
        .border_2()
        .border_color(rgb(if selected { theme.accent } else { theme.line }))
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, window, cx| this.select(index, window, cx)))
        .child(match picture {
            Some(picture) => img(picture)
                .size_full()
                .rounded_md()
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None => svg()
                .path(kind_icon(file.kind).path())
                .size(px(22.0))
                .text_color(rgb(theme.muted))
                .into_any_element(),
        })
        .child(
            div()
                .id(("remove-staged", index))
                .absolute()
                .top(px(-6.0))
                .right(px(-6.0))
                .size(px(20.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(rgb(theme.raised))
                .border_1()
                .border_color(rgb(theme.line))
                .invisible()
                .group_hover(group, Styled::visible)
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.remove(index, window, cx);
                }))
                .child(
                    svg()
                        .path(Icon::Close.path())
                        .size(px(11.0))
                        .text_color(rgb(theme.text)),
                ),
        )
}

fn kind_icon(kind: ContentKind) -> Icon {
    match kind {
        ContentKind::Photo => Icon::Camera,
        ContentKind::Video => Icon::Video,
        ContentKind::Audio | ContentKind::VoiceMessage => Icon::Waveform,
        _ => Icon::File,
    }
}

/// What the file will be sent as, and its size.
fn kind_line(file: &Staged) -> String {
    let kind = match file.kind {
        ContentKind::Audio | ContentKind::VoiceMessage => "Audio".to_owned(),
        ContentKind::Video => "Video".to_owned(),
        _ => file.path.extension().map_or_else(
            || "File".to_owned(),
            |extension| extension.to_string_lossy().to_uppercase(),
        ),
    };
    match file.size {
        Some(size) => format!("{kind} · {}", file_size(size)),
        None => kind,
    }
}
