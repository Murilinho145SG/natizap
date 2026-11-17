use gpui::{
    App, Bounds, Element, ElementId, ElementInputHandler, Entity, GlobalElementId,
    InspectorElementId, LayoutId, PaintQuad, Pixels, ShapedLine, Style, TextAlign, TextRun,
    UnderlineStyle, Window, fill, point, px, relative, rgb, rgba, size,
};

use crate::input::TextInput;
use crate::theme::{ACCENT, MUTED};

/// Selection highlight: the accent at a quarter opacity.
const SELECTION: u32 = (ACCENT << 8) | 0x40;

/// Lays out and paints a [`TextInput`]'s single line, caret and selection, and
/// connects it to the platform's text input.
pub(crate) struct TextElement {
    input: Entity<TextInput>,
}

impl TextElement {
    pub(crate) fn new(input: Entity<TextInput>) -> Self {
        Self { input }
    }
}

pub(crate) struct Prepaint {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let style = window.text_style();
        let (text, color) = if input.content.is_empty() {
            (input.placeholder.clone(), rgb(MUTED).into())
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
            Some(marked) => [
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
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect(),
            None => vec![run],
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(text, font_size, &runs, None);

        let selected = input.selected_range.clone();
        let (selection, cursor) = if selected.is_empty() {
            let x = line.x_for_index(input.cursor_offset());
            let caret = Bounds::new(
                point(bounds.left() + x, bounds.top()),
                size(px(2.), bounds.size.height),
            );
            (None, Some(fill(caret, rgb(ACCENT))))
        } else {
            let area = Bounds::from_corners(
                point(
                    bounds.left() + line.x_for_index(selected.start),
                    bounds.top(),
                ),
                point(
                    bounds.left() + line.x_for_index(selected.end),
                    bounds.bottom(),
                ),
            );
            (Some(fill(area, rgba(SELECTION))), None)
        };
        Prepaint {
            line: Some(line),
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
        let Some(line) = prepaint.line.take() else {
            return;
        };
        if let Err(error) = line.paint(
            bounds.origin,
            window.line_height(),
            TextAlign::Left,
            None,
            window,
            cx,
        ) {
            tracing::error!(%error, "could not paint text input");
        }
        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }
        self.input.update(cx, |input, _| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
        });
    }
}
