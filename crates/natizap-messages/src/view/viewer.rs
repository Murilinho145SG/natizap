//! Opening photos, videos and stickers large, and the window of a chat's
//! media, links and docs they can be opened from.

use gpui::{AnyElement, AppContext as _, Context, SharedString, Window};
use natizap_protocol::{ChatId, ContentKind};

use super::ConversationView;
use crate::items::MediaView;
use crate::media_browser::{BrowserClosed, MediaBrowser, MediaChosen, MediaUse, MessageWanted};
use crate::overview::MediaTile;

impl ConversationView {
    /// Shows a photo in the carousel, or a sticker large, downloading it
    /// if only its preview is here.
    pub(crate) fn view_picture(&mut self, media: &MediaView, cx: &mut Context<Self>) {
        if media.fetch.kind != ContentKind::Sticker {
            self.view_media(media, cx);
            return;
        }
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

    /// Opens an attachment: photos and videos in the carousel, anything
    /// else as its bubble would.
    fn show_media(&mut self, media: &MediaView, cx: &mut Context<Self>) {
        if media.fetch.kind == ContentKind::Photo {
            self.view_media(media, cx);
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
        let library = self.log.read(cx).library(chats.clone(), cx);
        let browser = cx.new(|cx| MediaBrowser::new(title, chats, library, cx));
        cx.subscribe(&browser, |this, _, MediaChosen(media, how), cx| match how {
            MediaUse::Open => this.show_media(media, cx),
            MediaUse::SaveAs => Self::save_media_as(media, cx),
            MediaUse::CopyImage => this.copy_media_image(media, cx),
        })
        .detach();
        cx.subscribe(&browser, |this, _, wanted: &MessageWanted, cx| {
            let MessageWanted { id, sent_at, chats } = wanted.clone();
            this.go_to(id, sent_at, chats, cx);
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

    /// The sticker being viewed large.
    pub(super) fn picture_viewer(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let media = self.viewing.as_ref()?;
        Some(self.sticker_viewer(media, window, cx))
    }
}
