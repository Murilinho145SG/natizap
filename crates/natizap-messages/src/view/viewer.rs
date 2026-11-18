//! Photos shown large over the dimmed window, and the window of a chat's
//! media, links and docs they can be opened from.

use gpui::prelude::*;
use gpui::{Context, ImageSource, ObjectFit, SharedString, Window, img, px};
use natizap_protocol::{ChatId, ContentKind};
use natizap_ui::dialog;

use super::ConversationView;
use crate::items::MediaView;
use crate::media_browser::{BrowserClosed, MediaBrowser, MediaChosen};
use crate::overview::MediaTile;

/// How much of the window a photo may take.
const COVERAGE: f32 = 0.9;

impl ConversationView {
    /// Shows `media`'s photo large, downloading the full photo if only its
    /// preview is here.
    pub(crate) fn view_picture(&mut self, media: &MediaView, cx: &mut Context<Self>) {
        if !media.file.as_deref().is_some_and(std::path::Path::exists) {
            let fetch = media.fetch.clone();
            self.log.update(cx, |log, cx| log.download(fetch, cx));
        }
        self.viewing = Some(media.clone());
        cx.notify();
    }

    /// Opens a photo or video from a chat's profile.
    pub fn show_tile(&mut self, tile: &MediaTile, cx: &mut Context<Self>) {
        if let Some(media) = &tile.media {
            self.show_media(media, cx);
        }
    }

    /// Opens an attachment: photos large, anything else as its bubble
    /// would.
    fn show_media(&mut self, media: &MediaView, cx: &mut Context<Self>) {
        if media.fetch.kind == ContentKind::Photo {
            self.view_picture(media, cx);
        } else {
            self.open_media(media, cx);
        }
    }

    /// Opens the window of everything `chats` shared: media, docs and
    /// links, under the chat's `title`.
    pub fn browse_media(
        &mut self,
        chats: Vec<ChatId>,
        title: SharedString,
        cx: &mut Context<Self>,
    ) {
        let library = self.log.read(cx).library(chats, cx);
        let browser = cx.new(|cx| MediaBrowser::new(title, library, cx));
        cx.subscribe(&browser, |this, _, MediaChosen(media), cx| {
            this.show_media(media, cx);
        })
        .detach();
        cx.subscribe(&browser, |this, _, _: &BrowserClosed, cx| {
            this.browsing = None;
            this.refocus = true;
            cx.notify();
        })
        .detach();
        self.browsing = Some(browser);
        cx.notify();
    }

    /// The photo being viewed, fitted to the window; a click closes it.
    pub(super) fn picture_viewer(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let media = self.viewing.as_ref()?;
        // A photo downloaded since it opened replaces its preview.
        let file = media
            .file
            .clone()
            .filter(|file| file.exists())
            .or_else(|| self.log.read(cx).saved_file(&media.fetch));
        let source: ImageSource = match (file, &media.preview) {
            (Some(file), _) => file.into(),
            (None, Some(preview)) => preview.clone().into(),
            (None, None) => return None,
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
