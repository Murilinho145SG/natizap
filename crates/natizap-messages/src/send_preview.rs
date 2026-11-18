//! Files about to be sent, shown before they go: dropped, pasted or picked
//! files gather here, each with a caption of its own, and Enter sends them
//! all, as the phone's preview before sending does.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    Context, Entity, EventEmitter, FocusHandle, Focusable as _, Image, ImageFormat,
    PathPromptOptions, SharedString, Window,
};
use natizap_protocol::ContentKind;
use natizap_ui::{FilesPasted, Submitted, TextInput};

use crate::outbox::kind_of;

mod render;

/// The most files sent at once, as on the phone.
pub(crate) const MOST_FILES: usize = 30;
/// Width the still of a video is drawn at.
const STILL_WIDTH: u32 = 960;

/// The user sent the files: each with its caption, as documents when
/// `as_document`.
#[derive(Debug, Clone)]
pub(crate) struct PreviewSent {
    pub(crate) files: Vec<(PathBuf, Option<String>)>,
    pub(crate) as_document: bool,
}

/// The user closed the preview, or removed every file from it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PreviewClosed;

/// One file about to be sent.
#[derive(Debug, Clone)]
struct Staged {
    path: PathBuf,
    name: SharedString,
    kind: ContentKind,
    caption: String,
    /// How big it is, once read.
    size: Option<u64>,
    /// A video's picture, once decoded.
    still: Option<Arc<Image>>,
}

/// The files about to be sent and their captions.
#[derive(Debug)]
pub(crate) struct SendPreview {
    files: Vec<Staged>,
    selected: usize,
    caption: Entity<TextInput>,
    as_document: bool,
}

impl EventEmitter<PreviewSent> for SendPreview {}
impl EventEmitter<PreviewClosed> for SendPreview {}

impl SendPreview {
    /// Shows `paths` before sending them, as documents when `as_document`,
    /// the first captioned `caption`.
    pub(crate) fn new(
        paths: Vec<PathBuf>,
        as_document: bool,
        caption: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| TextInput::new("Add a caption", cx));
        cx.subscribe(&input, |this, _, _: &Submitted, cx| this.send(cx))
            .detach();
        cx.subscribe_in(&input, window, |_, _, pasted: &FilesPasted, _, cx| {
            Self::add_pasted(pasted, cx);
        })
        .detach();
        let mut preview = Self {
            files: Vec::new(),
            selected: 0,
            caption: input,
            as_document,
        };
        preview.add(paths, cx);
        if let Some(first) = preview.files.first_mut() {
            first.caption = caption;
        }
        preview.show_caption(window, cx);
        preview
    }

    /// The caption field, which takes the keyboard while the preview shows.
    pub(crate) fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.caption.focus_handle(cx)
    }

    /// Adds `paths`, skipping files already here and any past
    /// [`MOST_FILES`], and reads their sizes and video stills off the UI
    /// thread.
    pub(crate) fn add(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let staged: Vec<&Path> = self.files.iter().map(|file| file.path.as_path()).collect();
        let fresh = admitted(&staged, paths);
        if fresh.is_empty() {
            return;
        }
        for path in &fresh {
            let kind = if self.as_document {
                ContentKind::Document
            } else {
                kind_of(path).0
            };
            self.files.push(Staged {
                name: path
                    .file_name()
                    .map_or_default(|name| name.to_string_lossy().into_owned().into()),
                path: path.clone(),
                kind,
                caption: String::new(),
                size: None,
                still: None,
            });
        }
        let as_document = self.as_document;
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn(async move {
                    fresh
                        .into_iter()
                        .map(|path| {
                            let details = read_details(&path, as_document);
                            (path, details)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                for (path, (size, still)) in read {
                    if let Some(file) = this.files.iter_mut().find(|file| file.path == path) {
                        file.size = size;
                        file.still = still;
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Adds pasted files, writing pasted pictures to files first.
    fn add_pasted(pasted: &FilesPasted, cx: &mut Context<Self>) {
        let mut paths = pasted.paths.clone();
        let images = pasted.images.clone();
        cx.spawn(async move |this, cx| {
            let written = cx
                .background_spawn(async move { write_pasted(&images) })
                .await;
            paths.extend(written);
            let _ = this.update(cx, |this, cx| this.add(paths, cx));
        })
        .detach();
    }

    /// Asks for more files to send.
    fn add_more(cx: &mut Context<Self>) {
        let picking = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Add".into()),
        });
        cx.spawn(async move |this, cx| match picking.await {
            Ok(Ok(Some(paths))) => {
                let _ = this.update(cx, |this, cx| this.add(paths, cx));
            }
            Ok(Ok(None)) | Err(_) => {}
            Ok(Err(error)) => tracing::warn!(%error, "could not ask for files"),
        })
        .detach();
    }

    fn select(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index == self.selected || index >= self.files.len() {
            return;
        }
        self.keep_caption(cx);
        self.selected = index;
        self.show_caption(window, cx);
        cx.notify();
    }

    fn remove(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.files.len() {
            return;
        }
        self.keep_caption(cx);
        self.files.remove(index);
        if self.files.is_empty() {
            cx.emit(PreviewClosed);
            return;
        }
        if self.selected >= index && self.selected > 0 {
            self.selected -= 1;
        }
        self.show_caption(window, cx);
        cx.notify();
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        self.keep_caption(cx);
        let files = self
            .files
            .iter()
            .map(|file| {
                let caption = file.caption.trim();
                (
                    file.path.clone(),
                    (!caption.is_empty()).then(|| caption.to_owned()),
                )
            })
            .collect();
        cx.emit(PreviewSent {
            files,
            as_document: self.as_document,
        });
    }

    /// Keeps what the caption field says as the selected file's caption.
    fn keep_caption(&mut self, cx: &Context<Self>) {
        let text = self.caption.read(cx).text().to_owned();
        if let Some(file) = self.files.get_mut(self.selected) {
            file.caption = text;
        }
    }

    /// Puts the selected file's caption in the caption field.
    fn show_caption(&self, window: &mut Window, cx: &mut Context<Self>) {
        let caption = self
            .files
            .get(self.selected)
            .map_or_default(|file| file.caption.clone());
        self.caption.update(cx, |input, cx| {
            input.clear(window, cx);
            input.insert(&caption, window, cx);
        });
    }
}

/// Which of `paths` join the `staged` files: files not staged yet, as
/// many as fit under [`MOST_FILES`]. Folders cannot be sent.
fn admitted(staged: &[&Path], paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut fresh: Vec<PathBuf> = Vec::new();
    for path in paths {
        if staged.len() + fresh.len() >= MOST_FILES {
            break;
        }
        if path.is_file() && !staged.contains(&path.as_path()) && !fresh.contains(&path) {
            fresh.push(path);
        }
    }
    fresh
}

/// A file's size and, for a video, its picture. Blocks while it decodes.
fn read_details(path: &Path, as_document: bool) -> (Option<u64>, Option<Arc<Image>>) {
    let size = std::fs::metadata(path).ok().map(|metadata| metadata.len());
    let still = (!as_document && kind_of(path).0 == ContentKind::Video)
        .then(|| natizap_playback::still_jpeg(path, STILL_WIDTH))
        .and_then(|still| {
            still
                .inspect_err(|error| tracing::debug!(%error, "no still for a video to send"))
                .ok()
        })
        .map(|jpeg| Arc::new(Image::from_bytes(ImageFormat::Jpeg, jpeg)));
    (size, still)
}

/// Writes pasted pictures, which have no file, to files of their own.
pub(crate) fn write_pasted(images: &[Image]) -> Vec<PathBuf> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    images
        .iter()
        .enumerate()
        .filter_map(|(index, image)| {
            let extension = match image.format() {
                ImageFormat::Png => "png",
                ImageFormat::Jpeg => "jpg",
                ImageFormat::Webp => "webp",
                ImageFormat::Gif => "gif",
                ImageFormat::Bmp => "bmp",
                ImageFormat::Tiff => "tiff",
                ImageFormat::Svg => "svg",
                ImageFormat::Ico => "ico",
                ImageFormat::Pnm => "pnm",
            };
            let path =
                std::env::temp_dir().join(format!("natizap-pasted-{stamp}-{index}.{extension}"));
            match std::fs::write(&path, image.bytes()) {
                Ok(()) => Some(path),
                Err(error) => {
                    tracing::warn!(%error, "could not keep a pasted picture");
                    None
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{MOST_FILES, admitted};

    /// A folder of its own for one test, removed when dropped.
    struct Folder(PathBuf);

    impl Folder {
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Real files, since folders and missing paths are turned away.
    fn files(count: usize) -> std::io::Result<(Folder, Vec<PathBuf>)> {
        let dir = std::env::temp_dir().join(format!(
            "natizap-send-preview-{}-{count}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir)?;
        let mut paths = Vec::new();
        for index in 0..count {
            let path = dir.join(format!("photo-{index}.jpg"));
            std::fs::write(&path, b"jpeg")?;
            paths.push(path);
        }
        Ok((Folder(dir), paths))
    }

    #[test]
    fn skips_files_already_staged_or_given_twice() -> std::io::Result<()> {
        let (_dir, paths) = files(2)?;
        let staged: Vec<&Path> = vec![paths[0].as_path()];
        let given = vec![paths[0].clone(), paths[1].clone(), paths[1].clone()];
        assert_eq!(admitted(&staged, given), vec![paths[1].clone()]);
        Ok(())
    }

    #[test]
    fn stops_at_the_most_files_sent_at_once() -> std::io::Result<()> {
        let (_dir, paths) = files(MOST_FILES + 5)?;
        assert_eq!(admitted(&[], paths).len(), MOST_FILES);
        Ok(())
    }

    #[test]
    fn turns_away_folders_and_missing_files() -> std::io::Result<()> {
        let (dir, mut paths) = files(1)?;
        paths.push(dir.path().to_owned());
        paths.push(dir.path().join("gone.jpg"));
        assert_eq!(admitted(&[], paths).len(), 1);
        Ok(())
    }
}
