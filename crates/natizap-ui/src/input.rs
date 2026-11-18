//! A text field, on one line or wrapping onto several.
//!
//! Adapted from GPUI's `input` example (Apache-2.0): editing follows the
//! platform input handler protocol (see `input_handler`), so IME
//! composition and the emoji picker work.

use std::ops::Range;

use gpui::prelude::*;
use gpui::{
    App, ClipboardEntry, ClipboardItem, Context, CursorStyle, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, ScrollEvent, SharedString, Window, div, point, px,
};

use crate::copied_files::copied_paths;
use crate::input_element::TextElement;
use crate::input_keys::{
    Backspace, Copy, Cut, Delete, DeleteWordLeft, DeleteWordRight, Down, End, Home, KEY_CONTEXT,
    Left, Newline, Paste, Right, SelectAll, SelectLeft, SelectRight, SelectWordLeft,
    SelectWordRight, Submit, Up, WordLeft, WordRight,
};
use crate::input_layout::TextLayout;
use crate::text_boundary::{next_grapheme, next_word, previous_grapheme, previous_word};

/// The user pressed Enter in a [`TextInput`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submitted(pub String);

/// A text field, on one line unless made [`wrapping`](TextInput::wrapping).
/// Emits [`Submitted`] on Enter; the owner decides whether to
/// [`clear`](TextInput::clear) it.
#[derive(Debug)]
pub struct TextInput {
    pub(crate) focus_handle: FocusHandle,
    pub(crate) content: SharedString,
    pub(crate) placeholder: SharedString,
    pub(crate) selected_range: Range<usize>,
    pub(crate) selection_reversed: bool,
    pub(crate) marked_range: Option<Range<usize>>,
    pub(crate) wraps: bool,
    /// The text as last painted, for mapping the pointer and the IME.
    pub(crate) layout: Option<TextLayout>,
    /// How far the text is scrolled to keep the caret in view.
    pub(crate) scroll: Point<Pixels>,
    is_selecting: bool,
}

impl EventEmitter<Submitted> for TextInput {}

/// The user pasted copied files or pictures into a [`TextInput`]; the
/// owner decides what to do with them.
#[derive(Debug, Clone, Default)]
pub struct FilesPasted {
    /// Copied files.
    pub paths: Vec<std::path::PathBuf>,
    /// Copied pictures, such as screenshots.
    pub images: Vec<gpui::Image>,
}

impl EventEmitter<FilesPasted> for TextInput {}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TextInput {
    /// An empty field showing `placeholder` until something is typed.
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: SharedString::default(),
            placeholder: placeholder.into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            wraps: false,
            layout: None,
            scroll: Point::default(),
            is_selecting: false,
        }
    }

    /// Wraps long text onto new lines, growing a few lines tall before it
    /// scrolls, instead of scrolling sideways on one line.
    #[must_use]
    pub fn wrapping(mut self) -> Self {
        self.wraps = true;
        self
    }

    /// The current text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.content
    }

    /// Types `text` at the cursor, replacing any selection.
    pub fn insert(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_text_in_range(None, text, window, cx);
    }

    /// Empties the field.
    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.content = SharedString::default();
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.marked_range = None;
        window.notify_text_input_changed(&self.focus_handle);
        cx.notify();
    }

    fn submit(&mut self, _: &Submit, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(Submitted(self.content.to_string()));
    }

    /// Starts a new line in a wrapping field; a one-line field has none.
    fn newline(&mut self, _: &Newline, window: &mut Window, cx: &mut Context<Self>) {
        if self.wraps {
            self.replace_text_in_range(None, "\n", window, cx);
        } else {
            cx.propagate();
        }
    }

    fn left(&mut self, _: &Left, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(
                previous_grapheme(&self.content, self.cursor_offset()),
                window,
                cx,
            );
        } else {
            self.move_to(self.selected_range.start, window, cx);
        }
    }

    fn right(&mut self, _: &Right, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(
                next_grapheme(&self.content, self.selected_range.end),
                window,
                cx,
            );
        } else {
            self.move_to(self.selected_range.end, window, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.select_to(
            previous_grapheme(&self.content, self.cursor_offset()),
            window,
            cx,
        );
    }

    fn select_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        self.select_to(
            next_grapheme(&self.content, self.cursor_offset()),
            window,
            cx,
        );
    }

    fn word_left(&mut self, _: &WordLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(
            previous_word(&self.content, self.cursor_offset()),
            window,
            cx,
        );
    }

    fn word_right(&mut self, _: &WordRight, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(next_word(&self.content, self.cursor_offset()), window, cx);
    }

    fn select_word_left(
        &mut self,
        _: &SelectWordLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(
            previous_word(&self.content, self.cursor_offset()),
            window,
            cx,
        );
    }

    fn select_word_right(
        &mut self,
        _: &SelectWordRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(next_word(&self.content, self.cursor_offset()), window, cx);
    }

    /// Moves the caret one wrapped line up or down, keeping its column. A
    /// one-line field leaves the keys to its owner, such as a list below a
    /// search field.
    fn move_vertically(&mut self, rows: f32, window: &mut Window, cx: &mut Context<Self>) {
        let target = match self.layout.as_ref() {
            Some(layout) if self.wraps => layout.index_rows_away(self.cursor_offset(), rows),
            _ => {
                cx.propagate();
                return;
            }
        };
        self.move_to(target, window, cx);
    }

    fn up(&mut self, _: &Up, window: &mut Window, cx: &mut Context<Self>) {
        self.move_vertically(-1.0, window, cx);
    }

    fn down(&mut self, _: &Down, window: &mut Window, cx: &mut Context<Self>) {
        self.move_vertically(1.0, window, cx);
    }

    fn select_all(&mut self, _: &SelectAll, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, window, cx);
        self.select_to(self.content.len(), window, cx);
    }

    fn home(&mut self, _: &Home, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, window, cx);
    }

    fn end(&mut self, _: &End, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), window, cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(
                previous_grapheme(&self.content, self.cursor_offset()),
                window,
                cx,
            );
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_word_left(
        &mut self,
        _: &DeleteWordLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_range.is_empty() {
            self.select_to(
                previous_word(&self.content, self.cursor_offset()),
                window,
                cx,
            );
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_word_right(
        &mut self,
        _: &DeleteWordRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_range.is_empty() {
            self.select_to(next_word(&self.content, self.cursor_offset()), window, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(
                next_grapheme(&self.content, self.cursor_offset()),
                window,
                cx,
            );
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        // Copied files and pictures go to the owner, not into the text: a
        // file manager also offers the files' paths as text.
        let mut pasted = FilesPasted::default();
        for entry in item.entries() {
            match entry {
                ClipboardEntry::ExternalPaths(paths) => {
                    pasted.paths.extend(paths.paths().iter().cloned());
                }
                ClipboardEntry::Image(image) => pasted.images.push(image.clone()),
                ClipboardEntry::String(_) => {}
            }
        }
        let text = item.text();
        if pasted.paths.is_empty() && pasted.images.is_empty() {
            // GPUI hands over a file manager's copied files only as their
            // locations, in text; text naming nothing but existing files
            // is taken as the files.
            if let Some(paths) = text.as_deref().and_then(copied_paths)
                && paths.iter().all(|path| path.is_file())
            {
                pasted.paths = paths;
            }
        }
        if !pasted.paths.is_empty() || !pasted.images.is_empty() {
            cx.emit(pasted);
        } else if let Some(text) = text {
            let text = text.replace("\r\n", "\n");
            let text = if self.wraps {
                text
            } else {
                text.replace('\n', " ")
            };
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            let selected = self.content[self.selected_range.clone()].to_string();
            cx.write_to_clipboard(ClipboardItem::new_string(selected));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        self.copy(&Copy, window, cx);
        if !self.selected_range.is_empty() {
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_selecting = true;
        let offset = self.index_for_mouse_position(event.position);
        if event.modifiers.shift {
            self.select_to(offset, window, cx);
        } else {
            self.move_to(offset, window, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), window, cx);
        }
    }

    /// Scrolls text that overflows the field: down a wrapping field, along a
    /// one-line one. Once the text is at its end, the wheel goes on to
    /// whatever holds the field.
    fn on_scroll(&mut self, event: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(layout) = self.layout.as_mut() else {
            return;
        };
        let delta = event.delta.pixel_delta(layout.line_height);
        let delta = if self.wraps {
            point(px(0.), delta.y)
        } else if delta.x == px(0.) {
            // A plain mouse wheel only turns vertically.
            point(delta.y, px(0.))
        } else {
            point(delta.x, px(0.))
        };
        layout.scroll_to(self.scroll - delta);
        let scroll = layout.scroll();
        if scroll != self.scroll {
            self.scroll = scroll;
            cx.stop_propagation();
            cx.notify();
        }
    }

    /// How far the text has scrolled down, and how far it can.
    pub(crate) fn vertical_scroll(&self) -> (Pixels, Pixels) {
        self.layout
            .as_ref()
            .map_or_default(|layout| (self.scroll.y, layout.max_scroll_y()))
    }

    /// How tall the field shows the text.
    pub(crate) fn viewport_height(&self) -> Pixels {
        self.layout
            .as_ref()
            .map_or_default(TextLayout::viewport_height)
    }

    /// Scrolls the text `offset` down from its top, as its scrollbar is
    /// dragged.
    pub(crate) fn scroll_down_to(&mut self, offset: Pixels, cx: &mut Context<Self>) {
        if let Some(layout) = self.layout.as_mut() {
            layout.scroll_to(point(self.scroll.x, offset));
            self.scroll = layout.scroll();
            cx.notify();
        }
    }

    fn move_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    pub(crate) fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        match self.layout.as_ref() {
            Some(layout) if !self.content.is_empty() => layout.index_for_position(position),
            _ => 0,
        }
    }

    pub(crate) fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8 = 0;
        let mut utf16 = 0;
        for ch in self.content.chars() {
            if utf16 >= offset {
                break;
            }
            utf16 += ch.len_utf16();
            utf8 += ch.len_utf8();
        }
        utf8
    }

    pub(crate) fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16 = 0;
        let mut utf8 = 0;
        for ch in self.content.chars() {
            if utf8 >= offset {
                break;
            }
            utf8 += ch.len_utf8();
            utf16 += ch.len_utf16();
        }
        utf16
    }

    pub(crate) fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    pub(crate) fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }

    pub(crate) fn splice(&mut self, range: &Range<usize>, new_text: &str) {
        self.content = format!(
            "{}{new_text}{}",
            &self.content[..range.start],
            &self.content[range.end..]
        )
        .into();
    }
}

impl Render for TextInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .w_full()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_word_left))
            .on_action(cx.listener(Self::delete_word_right))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::newline))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll(cx.listener(Self::on_scroll))
            .child(TextElement::new(cx.entity()))
    }
}
