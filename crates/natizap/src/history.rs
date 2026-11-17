//! Where back and forward lead, as in a browser: the chats visited before
//! and after the one on screen.

/// Chats visited, around the current one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct History<T> {
    back: Vec<T>,
    current: Option<T>,
    forward: Vec<T>,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self {
            back: Vec::new(),
            current: None,
            forward: Vec::new(),
        }
    }
}

impl<T: PartialEq> History<T> {
    /// The user went to `place` on their own: it becomes current, and what
    /// was ahead is forgotten.
    pub(crate) fn visit(&mut self, place: T) {
        if self.current.as_ref() == Some(&place) {
            return;
        }
        if let Some(current) = self.current.replace(place) {
            self.back.push(current);
        }
        self.forward.clear();
    }

    /// Steps back, returning the place to show; `None` at the start.
    pub(crate) fn back(&mut self) -> Option<&T> {
        let previous = self.back.pop()?;
        if let Some(current) = self.current.replace(previous) {
            self.forward.push(current);
        }
        self.current.as_ref()
    }

    /// Steps forward again, returning the place to show; `None` when
    /// nothing was stepped back from.
    pub(crate) fn forward(&mut self) -> Option<&T> {
        let next = self.forward.pop()?;
        if let Some(current) = self.current.replace(next) {
            self.back.push(current);
        }
        self.current.as_ref()
    }

    /// Forgets `place`, e.g. a chat that no longer exists.
    pub(crate) fn forget(&mut self, place: &T) {
        self.back.retain(|kept| kept != place);
        self.forward.retain(|kept| kept != place);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goes_back_and_forward_like_a_browser() {
        let mut history = History::default();
        history.visit("a");
        history.visit("b");
        history.visit("c");
        assert_eq!(history.back(), Some(&"b"));
        assert_eq!(history.back(), Some(&"a"));
        assert_eq!(history.back(), None);
        assert_eq!(history.forward(), Some(&"b"));
        assert_eq!(history.forward(), Some(&"c"));
        assert_eq!(history.forward(), None);
    }

    #[test]
    fn visiting_after_going_back_drops_what_was_ahead() {
        let mut history = History::default();
        history.visit("a");
        history.visit("b");
        history.back();
        history.visit("c");
        assert_eq!(history.forward(), None);
        assert_eq!(history.back(), Some(&"a"));
    }

    #[test]
    fn revisiting_the_current_place_adds_no_step() {
        let mut history = History::default();
        history.visit("a");
        history.visit("a");
        assert_eq!(history.back(), None);
    }
}
