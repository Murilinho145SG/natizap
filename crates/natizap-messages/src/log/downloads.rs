//! Downloading attachments, opening them, and taking stills of videos.

use std::path::{Path, PathBuf};

use gpui::{AppContext as _, Context, Task};
use natizap_playback::save_poster;
use natizap_protocol::ContentKind;

use super::{Input, MessageLog, PlayRequested, SoundReady};
use crate::media::{Fetch, PosterJob, PosterSource};
use crate::message::MessageChange;

impl MessageLog {
    /// Opens an attachment, downloading it first if needed.
    pub(crate) fn open_media(&mut self, fetch: Fetch, file: Option<&Path>, cx: &mut Context<Self>) {
        match file.filter(|file| file.exists()) {
            Some(file) => self.reveal(fetch.kind, fetch.message_id, file.to_owned(), cx),
            None => self.fetch(fetch, true, cx),
        }
    }

    /// Downloads a photo to be shown large, if it is not here yet.
    pub(crate) fn download(&mut self, fetch: Fetch, cx: &mut Context<Self>) {
        if self.saved_file(&fetch).is_none() {
            self.fetch(fetch, false, cx);
        }
    }

    /// Where `fetch`'s attachment was downloaded to, once it was.
    pub(crate) fn saved_file(&self, fetch: &Fetch) -> Option<PathBuf> {
        Some(fetch.path_in(&self.media_dir)).filter(|path| path.exists())
    }

    /// Videos play over the window and sound in its bubble; other files open
    /// in the system's app for them.
    fn reveal(&self, kind: ContentKind, message_id: String, file: PathBuf, cx: &mut Context<Self>) {
        match kind {
            ContentKind::Video => {
                let chat = self.open.as_ref().map_or_default(|open| open.title.clone());
                let title = format!("{} · {chat}", kind.label()).into();
                cx.emit(PlayRequested { file, title });
            }
            ContentKind::VoiceMessage | ContentKind::Audio => {
                cx.emit(SoundReady { message_id, file });
            }
            _ => cx.open_with_system(&file),
        }
    }

    /// Downloads an attachment to the media folder, then records where it
    /// went and, if asked, opens it.
    pub(super) fn fetch(&mut self, fetch: Fetch, open: bool, cx: &mut Context<Self>) {
        if !self.fetching.insert(fetch.message_id.clone()) {
            return;
        }
        let path = fetch.path_in(&self.media_dir);
        let downloading = self.protocol.read(cx).download(fetch.kind, fetch.media, cx);
        let (message_id, kind) = (fetch.message_id, fetch.kind);
        cx.spawn(async move |this, cx| {
            let saved = match downloading.await {
                Ok(bytes) => {
                    let path = path.clone();
                    cx.background_spawn(async move { save(&path, &bytes) })
                        .await
                }
                Err(error) => Err(error.to_string()),
            };
            let _ = this.update(cx, |log, cx| {
                match saved {
                    Ok(()) => {
                        log.fetching.remove(&message_id);
                        log.send_input(Input::from(MessageChange::MediaSaved {
                            message_id: message_id.clone(),
                            file: path.clone(),
                        }));
                        if open {
                            log.reveal(kind, message_id, path, cx);
                        }
                    }
                    // Stays in `fetching` so a failing file is not retried on
                    // every refresh; a click retries it after a restart.
                    Err(error) => tracing::warn!(%error, "could not download an attachment"),
                }
            });
        })
        .detach();
    }

    /// Takes a still from a downloaded video, then records where it went.
    /// Takes a still from a downloaded video, or downloads the sharper
    /// preview WhatsApp keeps, then records where it went.
    pub(super) fn make_poster(&mut self, job: PosterJob, cx: &mut Context<Self>) {
        // Also keeps a still that cannot be made from being retried on every
        // refresh; a restart retries it.
        if !self.posters.insert(job.fetch.message_id.clone()) {
            return;
        }
        let poster = job.fetch.poster_path_in(&self.media_dir);
        let file = poster.clone();
        let making: Task<Result<(), String>> = match job.source {
            PosterSource::Video(video) => cx.background_spawn(async move {
                save_poster(&video, &file).map_err(|error| error.to_string())
            }),
            PosterSource::HdThumbnail(media) => {
                let downloading = self
                    .protocol
                    .read(cx)
                    .download(ContentKind::Video, *media, cx);
                cx.spawn(async move |_, cx| {
                    let bytes = downloading.await.map_err(|error| error.to_string())?;
                    cx.background_spawn(async move { save(&file, &bytes) })
                        .await
                })
            }
        };
        let message_id = job.fetch.message_id;
        cx.spawn(async move |this, cx| match making.await {
            Ok(()) => {
                let _ = this.update(cx, |log, _| {
                    log.send_input(Input::from(MessageChange::PosterSaved {
                        message_id,
                        file: poster,
                    }));
                });
            }
            Err(error) => tracing::warn!(%error, "could not make a still of a video"),
        })
        .detach();
    }
}

fn save(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    std::fs::write(path, bytes).map_err(|error| error.to_string())
}
