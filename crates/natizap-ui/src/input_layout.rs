//! Where a [`TextInput`]'s text sits on screen: its paragraphs, each wrapped
//! onto lines, and the scroll that keeps the caret in view.

use gpui::{
    App, Bounds, PaintQuad, Pixels, Point, SharedString, Size, TextAlign, TextRun, UnderlineStyle,
    Window, WrappedLine, fill, point, px, rgb, rgba, size,
};

use crate::input::TextInput;
use crate::theme::Theme;

pub(crate) const CARET_WIDTH: Pixels = px(2.);

/// One line of the text as typed, between line breaks, wrapped onto as
/// many rows as it needs.
#[derive(Debug)]
struct Paragraph {
    /// Byte offset of its first character in the whole text.
    start: usize,
    /// Distance from the top of the text to its first row.
    top: Pixels,
    line: WrappedLine,
}

impl Paragraph {
    fn height(&self, line_height: Pixels) -> Pixels {
        self.line.size(line_height).height
    }
}

/// A [`TextInput`]'s text laid out in its bounds: where each byte offset
/// sits on screen, given the field's place and scroll.
#[derive(Debug)]
pub(crate) struct TextLayout {
    /// The text laid out: the placeholder while the field is empty.
    text: SharedString,
    paragraphs: Vec<Paragraph>,
    bounds: Bounds<Pixels>,
    scroll: Point<Pixels>,
    pub(crate) line_height: Pixels,
    /// The caret's offset when laid out, to tell when it moves.
    cursor: usize,
}

impl TextLayout {
    /// Lays out `input`'s text in `bounds`, scrolled as it was last.
    pub(crate) fn new(
        input: &TextInput,
        bounds: Bounds<Pixels>,
        window: &Window,
        theme: Theme,
    ) -> Self {
        let (text, paragraphs) = shape(input, Some(bounds.size.width), window, theme);
        Self {
            text,
            paragraphs,
            bounds,
            scroll: input.scroll,
            line_height: window.line_height(),
            cursor: input.cursor_offset(),
        }
    }

    /// How many rows `input`'s text takes when wrapped at `width`.
    pub(crate) fn rows(
        input: &TextInput,
        width: Option<Pixels>,
        window: &Window,
        theme: Theme,
    ) -> usize {
        shape(input, width, window, theme)
            .1
            .iter()
            .map(|paragraph| paragraph.line.wrap_boundaries().len() + 1)
            .sum()
    }

    /// The text that was laid out: the placeholder while the field is empty.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// The caret's offset when laid out.
    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    /// How far the text is scrolled.
    pub(crate) fn scroll(&self) -> Point<Pixels> {
        self.scroll
    }

    /// How far the text can scroll down.
    pub(crate) fn max_scroll_y(&self) -> Pixels {
        (self.size().height - self.bounds.size.height).max(px(0.))
    }

    /// How tall the field shows the text.
    pub(crate) fn viewport_height(&self) -> Pixels {
        self.bounds.size.height
    }

    /// Scrolls to `scroll`, held within the text.
    pub(crate) fn scroll_to(&mut self, scroll: Point<Pixels>) {
        self.scroll = self.clamp(scroll);
    }

    /// Scrolls as little as needed to show the caret.
    pub(crate) fn follow_caret(&mut self) {
        let caret = self.local_position(self.cursor);
        let view = self.bounds.size;
        let follow = |scroll: Pixels, caret: Pixels, extent: Pixels, view: Pixels| {
            scroll.min(caret).max(caret + extent - view)
        };
        self.scroll_to(point(
            follow(self.scroll.x, caret.x, CARET_WIDTH, view.width),
            follow(self.scroll.y, caret.y, self.line_height, view.height),
        ));
    }

    /// The paragraph holding `index`; a line break belongs to the paragraph
    /// it ends.
    fn paragraph_at(&self, index: usize) -> Option<&Paragraph> {
        self.paragraphs
            .iter()
            .rev()
            .find(|paragraph| paragraph.start <= index)
    }

    /// Where `index` sits relative to the top left of the text, unscrolled.
    fn local_position(&self, index: usize) -> Point<Pixels> {
        self.paragraph_at(index)
            .and_then(|paragraph| {
                let within = paragraph
                    .line
                    .position_for_index(index - paragraph.start, self.line_height)?;
                Some(within + point(px(0.), paragraph.top))
            })
            .unwrap_or_default()
    }

    /// Where `index` sits on screen.
    pub(crate) fn position_for_index(&self, index: usize) -> Point<Pixels> {
        self.bounds.origin + self.local_position(index) - self.scroll
    }

    /// The byte offset nearest `position` on screen.
    pub(crate) fn index_for_position(&self, position: Point<Pixels>) -> usize {
        let local = position - self.bounds.origin + self.scroll;
        self.index_for_local(local)
    }

    /// The byte offset `rows` rows below `index` (above when negative), at
    /// the same column; past the first or last row, the start or end.
    pub(crate) fn index_rows_away(&self, index: usize, rows: f32) -> usize {
        let caret = self.local_position(index);
        let y = caret.y + self.line_height * (rows + 0.5);
        if y < px(0.) {
            0
        } else if y >= self.size().height {
            self.text.len()
        } else {
            self.index_for_local(point(caret.x, y))
        }
    }

    /// The screen area of `start..end`, up to the end of the first row it
    /// covers.
    pub(crate) fn bounds_for_range(&self, start: usize, end: usize) -> Bounds<Pixels> {
        let start = self.position_for_index(start);
        let mut end = self.position_for_index(end);
        if end.y > start.y {
            end = point(self.bounds.right(), start.y);
        }
        Bounds::from_corners(start, end + point(px(0.), self.line_height))
    }

    fn index_for_local(&self, local: Point<Pixels>) -> usize {
        let Some(paragraph) = self
            .paragraphs
            .iter()
            .find(|paragraph| local.y < paragraph.top + paragraph.height(self.line_height))
            .or(self.paragraphs.last())
        else {
            return 0;
        };
        // Below its last row GPUI answers with the start of the paragraph,
        // not its end; clamp onto its rows.
        let last_row = paragraph.height(self.line_height) - self.line_height * 0.5;
        let y = (local.y - paragraph.top).clamp(px(0.), last_row.max(px(0.)));
        let within = paragraph
            .line
            .closest_index_for_position(point(local.x, y), self.line_height)
            .unwrap_or_else(|index| index);
        paragraph.start + within
    }

    /// The size of the whole text.
    fn size(&self) -> Size<Pixels> {
        self.paragraphs
            .iter()
            .fold(Size::default(), |total, paragraph| {
                let line = paragraph.line.size(self.line_height);
                size(total.width.max(line.width), total.height + line.height)
            })
    }

    fn clamp(&self, scroll: Point<Pixels>) -> Point<Pixels> {
        let text = self.size();
        let view = self.bounds.size;
        let most = point(
            (text.width + CARET_WIDTH - view.width).max(px(0.)),
            (text.height - view.height).max(px(0.)),
        );
        point(
            scroll.x.clamp(px(0.), most.x),
            scroll.y.clamp(px(0.), most.y),
        )
    }

    /// The selection's highlight, one rectangle per row it covers.
    pub(crate) fn selection(&self, start: usize, end: usize, theme: Theme) -> Vec<PaintQuad> {
        let start = self.position_for_index(start);
        let end = self.position_for_index(end);
        let (left, right) = (self.bounds.left(), self.bounds.right());
        let mut rows = Vec::new();
        let mut top = start.y;
        while top <= end.y {
            let from = if top == start.y { start.x } else { left };
            let to = if top >= end.y { end.x } else { right };
            rows.push(fill(
                Bounds::from_corners(point(from, top), point(to, top + self.line_height)),
                rgba(theme.selection),
            ));
            top += self.line_height;
        }
        rows
    }

    /// Paints the text, scrolled.
    pub(crate) fn paint(&self, window: &mut Window, cx: &mut App) {
        for paragraph in &self.paragraphs {
            let origin = self.bounds.origin + point(px(0.), paragraph.top) - self.scroll;
            if let Err(error) =
                paragraph
                    .line
                    .paint(origin, self.line_height, TextAlign::Left, None, window, cx)
            {
                tracing::error!(%error, "could not paint text input");
            }
        }
    }
}

/// Shapes the text `input` shows, its paragraphs wrapped at `wrap_width`
/// when it wraps.
fn shape(
    input: &TextInput,
    wrap_width: Option<Pixels>,
    window: &Window,
    theme: Theme,
) -> (SharedString, Vec<Paragraph>) {
    let style = window.text_style();
    let (text, color): (SharedString, _) = if input.content.is_empty() {
        (input.placeholder.clone(), rgb(theme.muted).into())
    } else {
        (input.content.clone(), style.color)
    };
    let run = TextRun {
        len: text.len(),
        font: style.font(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let runs = match input.marked_range.as_ref() {
        // IME composition in progress: underline the composing text.
        Some(marked) => vec![
            TextRun {
                len: marked.start,
                ..run.clone()
            },
            TextRun {
                len: marked.end - marked.start,
                underline: Some(UnderlineStyle {
                    color: Some(run.color),
                    thickness: px(1.0),
                    wavy: false,
                }),
                ..run.clone()
            },
            TextRun {
                len: text.len() - marked.end,
                ..run
            },
        ],
        None => vec![run],
    };
    let font_size = style.font_size.to_pixels(window.rem_size());
    let wrap_width = wrap_width.filter(|_| input.wraps);
    let line_height = window.line_height();
    let lines =
        match window
            .text_system()
            .shape_text(text.clone(), font_size, &runs, wrap_width, None)
        {
            Ok(lines) => lines,
            Err(error) => {
                tracing::error!(%error, "could not shape text input");
                return (text, Vec::new());
            }
        };
    // GPUI splits the text at line breaks and drops them: each line starts
    // one byte past the end of the one before.
    let mut start = 0;
    let mut top = px(0.);
    let paragraphs = lines
        .into_iter()
        .map(|line| {
            let paragraph = Paragraph { start, top, line };
            start += paragraph.line.len() + 1;
            top += paragraph.height(line_height);
            paragraph
        })
        .collect();
    (text, paragraphs)
}
