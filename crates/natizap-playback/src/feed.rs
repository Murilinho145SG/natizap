//! The newest picture of a live video, handed from the thread that makes
//! pictures to the view that draws them: older ones never drawn are
//! dropped, so a slow screen shows the present, not a backlog.

use std::sync::{Arc, Mutex, PoisonError};

use crate::Picture;

/// A live video's newest picture, to draw.
#[derive(Debug, Clone)]
pub struct VideoFeed {
    newest: Arc<Mutex<Option<Picture>>>,
    arrived: async_channel::Receiver<()>,
}

impl VideoFeed {
    /// The newest picture since the last one taken, if any.
    #[must_use]
    pub fn take(&self) -> Option<Picture> {
        self.newest
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }

    /// Ready each time a new picture arrived; closed once the video ended.
    #[must_use]
    pub fn arrivals(&self) -> async_channel::Receiver<()> {
        self.arrived.clone()
    }
}

/// The making end of a [`VideoFeed`].
#[derive(Debug)]
pub(crate) struct FeedWriter {
    newest: Arc<Mutex<Option<Picture>>>,
    arrive: async_channel::Sender<()>,
}

impl FeedWriter {
    /// Makes `picture` the newest, replacing one never taken.
    pub(crate) fn show(&self, picture: Picture) {
        *self.newest.lock().unwrap_or_else(PoisonError::into_inner) = Some(picture);
        // Full means a wake-up is already waiting.
        let _ = self.arrive.try_send(());
    }
}

/// A feed and its making end.
pub(crate) fn feed() -> (FeedWriter, VideoFeed) {
    let newest: Arc<Mutex<Option<Picture>>> = Arc::default();
    let (arrive, arrived) = async_channel::bounded(1);
    (
        FeedWriter {
            newest: Arc::clone(&newest),
            arrive,
        },
        VideoFeed { newest, arrived },
    )
}

#[cfg(test)]
mod tests {
    use super::feed;
    use crate::Picture;

    fn picture(width: u32) -> Picture {
        Picture {
            width,
            height: 2,
            bgra: vec![0; width as usize * 8],
        }
    }

    #[test]
    fn shows_only_the_newest_picture_once() {
        let (writer, feed) = feed();
        writer.show(picture(2));
        writer.show(picture(4));
        assert_eq!(feed.take().map(|picture| picture.width), Some(4));
        assert_eq!(feed.take(), None);
        assert!(feed.arrivals().try_recv().is_ok());
    }
}
