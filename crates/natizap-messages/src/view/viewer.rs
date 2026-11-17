//! A photo shown large over the dimmed window.

use gpui::prelude::*;
use gpui::{Context, ImageSource, ObjectFit, Window, img, px};
use natizap_ui::dialog;

use super::ConversationView;
use crate::items::MediaView;

/// How much of the window a photo may take.
const COVERAGE: f32 = 0.9;

impl ConversationView {
    /// Shows `media`'s photo large.
    pub(crate) fn view_picture(&mut self, media: &MediaView, cx: &mut Context<Self>) {
        self.viewing = Some(media.clone());
        cx.notify();
    }

    /// The photo being viewed, fitted to the window; a click closes it.
    pub(super) fn picture_viewer(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let media = self.viewing.as_ref()?;
        let source: ImageSource = match (&media.file, &media.preview) {
            (Some(file), _) if file.exists() => file.clone().into(),
            (_, Some(preview)) => preview.clone().into(),
            _ => return None,
        };
        let viewport = window.viewport_size();
        let (width, height) = media.size;
        let scale = (f32::from(viewport.width) * COVERAGE / width)
            .min(f32::from(viewport.height) * COVERAGE / height);
        let view = cx.weak_entity();
        Some(dialog(
            window,
            img(source)
                .w(px(width * scale))
                .h(px(height * scale))
                .rounded(px(8.0))
                .object_fit(ObjectFit::Contain),
            move |_, cx| {
                let _ = view.update(cx, |this, cx| {
                    this.viewing = None;
                    cx.notify();
                });
            },
        ))
    }
}
