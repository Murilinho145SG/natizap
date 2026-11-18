//! Saving an attachment where the user picks, and copying a picture.

use gpui::Context;

use super::ConversationView;
use crate::items::MediaView;

impl ConversationView {
    /// Asks where to save `media`'s file, then saves it there, downloading
    /// it first if it is not here yet.
    pub(crate) fn save_media_as(media: &MediaView, cx: &mut Context<Self>) {
        let Some(dir) = dirs::download_dir().or_else(dirs::home_dir) else {
            tracing::warn!("no folder to offer for saving an attachment");
            return;
        };
        let picking = cx.prompt_for_new_path(&dir, Some(&media.fetch.save_name()));
        let (fetch, file) = (media.fetch.clone(), media.file.clone());
        cx.spawn(async move |this, cx| {
            let dest = match picking.await {
                Ok(Ok(Some(dest))) => dest,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    tracing::warn!(%error, "could not ask where to save an attachment");
                    return;
                }
            };
            let _ = this.update(cx, |this, cx| {
                this.log
                    .update(cx, |log, cx| log.save_as(fetch, file.as_deref(), dest, cx));
            });
        })
        .detach();
    }

    /// Puts `media`'s picture on the clipboard, downloading it first if it
    /// is not here yet.
    pub(crate) fn copy_media_image(&mut self, media: &MediaView, cx: &mut Context<Self>) {
        let (fetch, file) = (media.fetch.clone(), media.file.clone());
        self.log
            .update(cx, |log, cx| log.copy_image(fetch, file.as_deref(), cx));
    }
}
