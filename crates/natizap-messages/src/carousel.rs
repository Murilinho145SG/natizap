//! The open photo or video, and the chat's other photos and videos around
//! it, as the carousel walks through them.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{AnyView, FocusHandle, Pixels, Point, ScrollHandle, Task};
use natizap_protocol::ChatId;

use crate::items::MediaView;
use crate::library::Shelved;

/// Which way the carousel last moved, to slide the next item in from that
/// side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Moved {
    /// It just opened: nothing to slide from.
    Opened,
    /// To an older item, which comes in from the left.
    Older,
    /// To a newer item, which comes in from the right.
    Newer,
}

/// A step through the photos and videos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Toward {
    Older,
    Newer,
}

/// The carousel over a chat's photos and videos.
pub(crate) struct Carousel {
    /// Every address of the chat they came in.
    pub(crate) chats: Vec<ChatId>,
    /// The chat's photos and videos, newest first, once read.
    shelf: Arc<[Shelved]>,
    /// What shows: the item opened, until the shelf is read.
    current: MediaView,
    /// Where `current` is on the shelf, once read and found.
    at: Option<usize>,
    pub(crate) moved: Moved,
    /// Small copies made while it shows, by index into the shelf.
    pub(crate) made: HashMap<usize, PathBuf>,
    /// The video playing, by message id, as the app handed it over.
    player: Option<(String, AnyView)>,
    /// The video a player was last asked for, so it is asked once.
    pub(crate) asked: Option<String>,
    /// The strip of small pictures under the stage.
    pub(crate) strip: ScrollHandle,
    pub(crate) focus: FocusHandle,
    /// Where a right click opened its menu.
    pub(crate) menu: Option<Point<Pixels>>,
    _reading: Task<()>,
    pub(crate) making: Task<()>,
}

impl std::fmt::Debug for Carousel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Carousel")
            .field("current", &self.current.fetch.message_id)
            .field("at", &self.at)
            .finish_non_exhaustive()
    }
}

impl Carousel {
    /// Opens on `media`, among the photos and videos of `chats` once
    /// `reading` has read them.
    pub(crate) fn new(
        media: MediaView,
        chats: Vec<ChatId>,
        focus: FocusHandle,
        reading: Task<()>,
    ) -> Self {
        Self {
            chats,
            shelf: Arc::from([]),
            current: media,
            at: None,
            moved: Moved::Opened,
            made: HashMap::new(),
            player: None,
            asked: None,
            strip: ScrollHandle::new(),
            focus,
            menu: None,
            _reading: reading,
            making: Task::ready(()),
        }
    }

    /// What shows.
    pub(crate) fn current(&self) -> &MediaView {
        &self.current
    }

    /// The chat's photos and videos, newest first; empty until read.
    pub(crate) fn shelf(&self) -> &[Shelved] {
        &self.shelf
    }

    /// Where what shows is on the shelf.
    pub(crate) fn at(&self) -> Option<usize> {
        self.at
    }

    /// Takes the chat's photos and videos as read, finding what shows
    /// among them.
    pub(crate) fn shelve(&mut self, items: Vec<Shelved>) {
        self.at = position_of(&items, &self.current.fetch.message_id);
        self.shelf = Arc::from(items);
    }

    /// Whether there is an item `toward` from what shows.
    pub(crate) fn can_step(&self, toward: Toward) -> bool {
        self.at
            .and_then(|at| step(at, self.shelf.len(), toward))
            .is_some()
    }

    /// Moves one item `toward`, if there is one. Returns whether it moved.
    pub(crate) fn step(&mut self, toward: Toward) -> bool {
        match self.at.and_then(|at| step(at, self.shelf.len(), toward)) {
            Some(index) => self.go(index),
            None => false,
        }
    }

    /// Moves to the item at `index` on the shelf. Returns whether it moved.
    pub(crate) fn go(&mut self, index: usize) -> bool {
        let (Some(at), Some(item)) = (self.at, self.shelf.get(index)) else {
            return false;
        };
        if index == at {
            return false;
        }
        // Newer items sit earlier on the shelf.
        self.moved = if index < at {
            Moved::Newer
        } else {
            Moved::Older
        };
        self.current = item.media.clone();
        self.at = Some(index);
        self.player = None;
        self.asked = None;
        self.menu = None;
        true
    }

    /// The player for what shows, when it is a video being played.
    pub(crate) fn player(&self) -> Option<&AnyView> {
        self.player
            .as_ref()
            .filter(|(id, _)| *id == self.current.fetch.message_id)
            .map(|(_, player)| player)
    }

    /// Plays `player` if it is for what shows; a late answer for a video
    /// already moved past is dropped.
    pub(crate) fn play(&mut self, message_id: &str, player: AnyView) {
        if self.current.fetch.message_id == message_id {
            self.player = Some((message_id.to_owned(), player));
        }
    }
}

/// Where message `id`'s item is on `shelf`.
fn position_of(shelf: &[Shelved], id: &str) -> Option<usize> {
    shelf
        .iter()
        .position(|item| item.media.fetch.message_id == id)
}

/// The index one step `toward` from `at` on a shelf of `len` items, newest
/// first, if there is one.
fn step(at: usize, len: usize, toward: Toward) -> Option<usize> {
    match toward {
        Toward::Newer => at.checked_sub(1),
        Toward::Older => Some(at + 1).filter(|next| *next < len),
    }
}

#[cfg(test)]
mod tests {
    use super::{Toward, step};

    #[test]
    fn newer_goes_toward_the_front_of_the_shelf() {
        assert_eq!(step(2, 5, Toward::Newer), Some(1));
        assert_eq!(step(0, 5, Toward::Newer), None);
    }

    #[test]
    fn older_goes_toward_the_back_and_stops_at_the_oldest() {
        assert_eq!(step(2, 5, Toward::Older), Some(3));
        assert_eq!(step(4, 5, Toward::Older), None);
    }

    #[test]
    fn a_lone_item_goes_nowhere() {
        assert_eq!(step(0, 1, Toward::Newer), None);
        assert_eq!(step(0, 1, Toward::Older), None);
    }
}
