//! A live video on screen: each new picture of a feed made into an image
//! once, and painted where a view puts it, as large as fits with its shape
//! kept.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{Context, Corners, Div, ObjectFit, Pixels, RenderImage, Task, Window, canvas, div};
use image::{Frame, RgbaImage};
use natizap_playback::{Picture, VideoFeed};

/// A live video's newest picture, as an image to paint.
pub(crate) struct Screen {
    image: Option<Arc<RenderImage>>,
    _task: Task<()>,
}

impl Screen {
    /// Shows `feed`'s pictures as they come.
    pub(crate) fn new(feed: VideoFeed, cx: &mut Context<Self>) -> Self {
        let arrivals = feed.arrivals();
        let task = cx.spawn(async move |this, cx| {
            while arrivals.recv().await.is_ok() {
                let Some(image) = feed.take().and_then(render_image) else {
                    continue;
                };
                let shown = this.update(cx, |screen, cx| {
                    screen.image = Some(image);
                    cx.notify();
                });
                if shown.is_err() {
                    return;
                }
            }
        });
        Self {
            image: None,
            _task: task,
        }
    }

    /// The newest picture, once one came.
    pub(crate) fn image(&self) -> Option<Arc<RenderImage>> {
        self.image.clone()
    }
}

/// `picture` as an image GPUI paints; its BGRA bytes are what GPUI's
/// images hold, whatever the buffer's type says.
pub(crate) fn render_image(picture: Picture) -> Option<Arc<RenderImage>> {
    let buffer = RgbaImage::from_raw(picture.width, picture.height, picture.bgra)?;
    Some(Arc::new(RenderImage::new(vec![Frame::new(buffer)])))
}

/// What one window painted of a live video, so each picture it moves on
/// from leaves the window's GPU atlas instead of piling up there.
#[derive(Default)]
pub(crate) struct Painted(Option<Arc<RenderImage>>);

impl Painted {
    /// `image`, the picture to paint in `window` now, letting go of the
    /// one painted before.
    pub(crate) fn paint(
        &mut self,
        image: Option<Arc<RenderImage>>,
        corners: Pixels,
        window: &mut Window,
    ) -> Div {
        let replaced = match (&self.0, &image) {
            (Some(old), Some(new)) => !Arc::ptr_eq(old, new),
            (Some(_), None) => true,
            (None, _) => false,
        };
        if replaced
            && let Some(old) = self.0.take()
            && let Err(error) = window.drop_image(old)
        {
            tracing::debug!(%error, "could not free a video picture");
        }
        self.0.clone_from(&image);
        let Some(image) = image else {
            return div();
        };
        // Painted by hand: an image element sized in relative units is laid
        // out by the picture's own shape, so an upright picture would grow
        // past the space it was given.
        div().absolute().inset_0().child(
            canvas(
                |_, _, _| {},
                move |bounds, (), window, _| {
                    let fitted = ObjectFit::Contain.get_bounds(bounds, image.size(0));
                    if let Err(error) = window.paint_image(
                        fitted,
                        fitted,
                        Corners::all(corners),
                        Arc::clone(&image),
                        0,
                        false,
                    ) {
                        tracing::debug!(%error, "could not paint a video picture");
                    }
                },
            )
            .size_full(),
        )
    }
}
