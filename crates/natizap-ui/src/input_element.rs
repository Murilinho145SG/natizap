use gpui::{
    App, AvailableSpace, Bounds, ContentMask, Element, ElementId, ElementInputHandler, Entity,
    GlobalElementId, InspectorElementId, LayoutId, PaintQuad, Pixels, Size, Style, Window, fill,
    rgb, size,
};

use crate::input::TextInput;
use crate::input_layout::{CARET_WIDTH, TextLayout};
use crate::theme::ACCENT;

/// How many rows a wrapping field grows to before it scrolls, as on the
/// phone's composer.
const MOST_ROWS: u8 = 6;

/// Lays out and paints a [`TextInput`]'s text, caret and selection, and
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
    layout: Option<TextLayout>,
    cursor: Option<PaintQuad>,
    selection: Vec<PaintQuad>,
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
        style.size.width = gpui::relative(1.).into();
        let line_height = window.line_height();
        if !self.input.read(cx).wraps {
            style.size.height = line_height.into();
            return (window.request_layout(style, [], cx), ());
        }
        let input = self.input.clone();
        let layout_id = window.request_measured_layout(
            style,
            move |known: Size<Option<Pixels>>, available: Size<AvailableSpace>, window, cx| {
                let width = known.width.or(match available.width {
                    AvailableSpace::Definite(width) => Some(width),
                    AvailableSpace::MinContent | AvailableSpace::MaxContent => None,
                });
                let rows = TextLayout::rows(input.read(cx), width, window);
                let rows = u8::try_from(rows).unwrap_or(u8::MAX).min(MOST_ROWS);
                size(width.unwrap_or_default(), line_height * f32::from(rows))
            },
        );
        (layout_id, ())
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
        let mut layout = TextLayout::new(input, bounds, window);
        // Follow the caret only when it moved or the text changed, so the
        // text stays where the mouse wheel left it.
        let moved = input
            .layout
            .as_ref()
            .is_none_or(|last| last.cursor() != layout.cursor() || last.text() != layout.text());
        if moved {
            layout.follow_caret();
        } else {
            layout.scroll_to(input.scroll);
        }

        let selected = input.selected_range.clone();
        let (selection, cursor) = if selected.is_empty() {
            let caret = layout.position_for_index(input.cursor_offset());
            let caret = Bounds::new(caret, size(CARET_WIDTH, layout.line_height));
            (Vec::new(), Some(fill(caret, rgb(ACCENT))))
        } else {
            (layout.selection(selected.start, selected.end), None)
        };
        Prepaint {
            layout: Some(layout),
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
        let Some(layout) = prepaint.layout.take() else {
            return;
        };
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for selection in prepaint.selection.drain(..) {
                window.paint_quad(selection);
            }
            layout.paint(window, cx);
            if focus_handle.is_focused(window)
                && let Some(cursor) = prepaint.cursor.take()
            {
                window.paint_quad(cursor);
            }
        });
        self.input.update(cx, |input, _| {
            input.scroll = layout.scroll();
            input.layout = Some(layout);
        });
    }
}
