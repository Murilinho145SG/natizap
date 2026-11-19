//! Downloading attachments, opening, saving and copying them, and taking
//! stills of videos.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui::{AppContext as _, ClipboardItem, Context, Image, ImageFormat, Task};
use natizap_playback::save_poster;
use natizap_protocol::{ActionError, ContentKind};

use super::{Input, MessageLog, SoundReady};
use crate::media::{Fetch, PosterJob, PosterSource};
use crate::message::MessageChange;

/// A downloaded file, and the link it came from when its sender's phone
/// had to upload it again.
type Downloaded = (Vec<u8>, Option<String>);

/// What to do with an attachment once it is on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Then {
    /// Nothing: it was fetched to be shown.
    Keep,
    /// Open it, as a click on it asked.
    Open,
    /// Copy it to where the user chose.
    SaveTo(PathBuf),
    /// Put the picture on the clipboard.
    CopyImage,
}

impl MessageLog {
    /// Opens an attachment, downloading it first if needed.
    pub(crate) fn open_media(&mut self, fetch: Fetch, file: Option<&Path>, cx: &mut Context<Self>) {
        self.when_on_disk(fetch, file, Then::Open, cx);
    }

    /// Downloads a photo to be shown large, if it is not here yet.
    pub(crate) fn download(&mut self, fetch: Fetch, cx: &mut Context<Self>) {
        if self.saved_file(&fetch).is_none() {
            self.fetch(fetch, Then::Keep, cx);
        }
    }

    /// Copies an attachment to `dest`, downloading it first if needed.
    pub(crate) fn save_as(
        &mut self,
        fetch: Fetch,
        file: Option<&Path>,
        dest: PathBuf,
        cx: &mut Context<Self>,
    ) {
        self.when_on_disk(fetch, file, Then::SaveTo(dest), cx);
    }

    /// Puts a picture on the clipboard, downloading it first if needed.
    pub(crate) fn copy_image(&mut self, fetch: Fetch, file: Option<&Path>, cx: &mut Context<Self>) {
        self.when_on_disk(fetch, file, Then::CopyImage, cx);
    }

    /// Does `then` with `fetch`'s file: at once when `file`, or the copy
    /// in the media folder, is on disk; after downloading it otherwise.
    fn when_on_disk(
        &mut self,
        fetch: Fetch,
        file: Option<&Path>,
        then: Then,
        cx: &mut Context<Self>,
    ) {
        let on_disk = file
            .filter(|file| file.exists())
            .map(Path::to_owned)
            .or_else(|| self.saved_file(&fetch));
        match on_disk {
            Some(file) => Self::finish(fetch.kind, fetch.message_id, file, then, cx),
            None => self.fetch(fetch, then, cx),
        }
    }

    /// Does `then` with `file`, the attachment of message `message_id`.
    fn finish(
        kind: ContentKind,
        message_id: String,
        file: PathBuf,
        then: Then,
        cx: &mut Context<Self>,
    ) {
        match then {
            Then::Keep => {}
            Then::Open => Self::reveal(kind, message_id, file, cx),
            Then::SaveTo(dest) => {
                cx.background_spawn(async move {
                    if let Err(error) = std::fs::copy(&file, &dest) {
                        tracing::warn!(%error, dest = %dest.display(), "could not save an attachment");
                    }
                })
                .detach();
            }
            Then::CopyImage => {
                let reading = cx.background_spawn(async move { std::fs::read(&file) });
                cx.spawn(async move |_, cx| match reading.await {
                    Ok(bytes) => {
                        let Some(format) = picture_format(&bytes) else {
                            tracing::warn!("cannot copy: the file is not a picture");
                            return;
                        };
                        let image = Image::from_bytes(format, bytes);
                        cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_image(&image)));
                    }
                    Err(error) => tracing::warn!(%error, "could not read a picture to copy"),
                })
                .detach();
            }
        }
    }

    /// Where `fetch`'s attachment was downloaded to, once it was.
    pub(crate) fn saved_file(&self, fetch: &Fetch) -> Option<PathBuf> {
        Some(fetch.path_in(&self.media_dir)).filter(|path| path.exists())
    }

    /// Sound plays in its bubble; other files open in the system's app for
    /// them. Photos and videos open in the carousel, never here.
    fn reveal(kind: ContentKind, message_id: String, file: PathBuf, cx: &mut Context<Self>) {
        match kind {
            ContentKind::VoiceMessage | ContentKind::Audio => {
                cx.emit(SoundReady { message_id, file });
            }
            _ => cx.open_with_system(&file),
        }
    }

    /// Downloads an attachment to the media folder, then records where it
    /// went and does `then` with it. A download already under way does
    /// `then` too when it lands.
    pub(super) fn fetch(&mut self, fetch: Fetch, then: Then, cx: &mut Context<Self>) {
        // One the user asked for is tried again even if it failed before,
        // as a tap on it does on the phone.
        if then != Then::Keep && self.failed.fetches.remove(&fetch.message_id) {
            self.fetching.remove(&fetch.message_id);
        }
        // Asking the phone to upload an expired file again is what a tap
        // does; done for every old attachment a chat shows, it floods
        // WhatsApp, which then refuses everything for a while.
        if then == Then::Keep && fetch.media.link_expired() {
            return;
        }
        if let Some(waiting) = self.fetching.get_mut(&fetch.message_id) {
            if then != Then::Keep {
                waiting.push(then);
            }
            return;
        }
        self.fetching.insert(fetch.message_id.clone(), vec![then]);
        let path = fetch.path_in(&self.media_dir);
        let protocol = self.protocol.read(cx);
        // An expired link cannot serve the file; the phone that sent it is
        // asked to upload it again, as a tap does in the phone app.
        let downloading: Task<Result<Downloaded, ActionError>> = if fetch.media.link_expired() {
            let recovering = protocol.recover(fetch.kind, fetch.media, fetch.origin, cx);
            cx.background_spawn(async move {
                let (bytes, media) = recovering.await?;
                Ok((bytes, Some(media.direct_path)))
            })
        } else {
            let downloading = protocol.download(fetch.kind, fetch.media, cx);
            cx.background_spawn(async move { Ok((downloading.await?, None)) })
        };
        let (message_id, kind) = (fetch.message_id, fetch.kind);
        cx.spawn(async move |this, cx| {
            let saved = match downloading.await {
                Ok((bytes, direct_path)) => {
                    let path = path.clone();
                    cx.background_spawn(async move { save(&path, &bytes).map(|()| direct_path) })
                        .await
                }
                Err(error) => Err(error.to_string()),
            };
            let _ = this.update(cx, |log, cx| {
                match saved {
                    Ok(direct_path) => {
                        let waiting = log.fetching.remove(&message_id).unwrap_or_default();
                        log.send_input(Input::from(MessageChange::MediaSaved {
                            message_id: message_id.clone(),
                            file: path.clone(),
                            direct_path,
                        }));
                        for then in waiting {
                            Self::finish(kind, message_id.clone(), path.clone(), then, cx);
                        }
                    }
                    // Stays in `fetching` so a failing file is not retried on
                    // every refresh, until the connection comes back; what
                    // waited on it is dropped.
                    Err(error) => {
                        tracing::warn!(%error, "could not download an attachment");
                        if let Some(waiting) = log.fetching.get_mut(&message_id) {
                            waiting.clear();
                        }
                        log.failed.fetches.insert(message_id);
                    }
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
        // refresh, until the connection comes back.
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
            Err(error) => {
                tracing::warn!(%error, "could not make a still of a video");
                let _ = this.update(cx, |log, _| {
                    log.failed.posters.insert(message_id);
                });
            }
        })
        .detach();
    }

    /// Tries again what failed: the connection is back. Reading the open
    /// chat anew fetches what it shows.
    pub(super) fn retry_failed(&mut self) {
        let failed = std::mem::take(&mut self.failed);
        if failed.fetches.is_empty() && failed.posters.is_empty() {
            return;
        }
        tracing::info!(
            downloads = failed.fetches.len(),
            stills = failed.posters.len(),
            "trying failed attachments again"
        );
        self.fetching.retain(|id, _| !failed.fetches.contains(id));
        self.posters.retain(|id| !failed.posters.contains(id));
        self.send_input(Input::Refresh);
    }
}

/// Downloads and stills that failed, by message id.
#[derive(Debug, Default)]
pub(super) struct Failed {
    fetches: HashSet<String>,
    posters: HashSet<String>,
}

/// The picture format of `bytes`, if the clipboard can carry it.
fn picture_format(bytes: &[u8]) -> Option<ImageFormat> {
    match image::guess_format(bytes).ok()? {
        image::ImageFormat::Png => Some(ImageFormat::Png),
        image::ImageFormat::Jpeg => Some(ImageFormat::Jpeg),
        image::ImageFormat::WebP => Some(ImageFormat::Webp),
        image::ImageFormat::Gif => Some(ImageFormat::Gif),
        image::ImageFormat::Bmp => Some(ImageFormat::Bmp),
        image::ImageFormat::Tiff => Some(ImageFormat::Tiff),
        _ => None,
    }
}

fn save(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    std::fs::write(path, bytes).map_err(|error| error.to_string())
}
