//! The list while a search is typed: the chats it matches, then the
//! messages of every chat holding it, which the app hands over.

use gpui::prelude::*;
use gpui::{AnyElement, AnyView, Context, Div, FontWeight, div, px, rgb};
use natizap_ui::theme::{ActiveTheme as _, Theme};

use super::{ChatListView, ROW_HEIGHT};
use crate::list::ChatRow;

/// The chats matching a search show at most this many rows, so the
/// messages below keep room.
const MOST_CHAT_ROWS: u8 = 4;

/// The user typed in the chat list's search field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryChanged(pub String);

impl ChatListView {
    /// Shows `results`, the messages holding what is typed, under the
    /// chats it matches while a search is typed.
    pub fn set_message_results(&mut self, results: AnyView, cx: &mut Context<Self>) {
        self.message_results = Some(results);
        cx.notify();
    }

    /// Tells the app what is typed, when it changed.
    pub(super) fn query_changed(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).text().trim().to_owned();
        if query != self.query {
            self.query.clone_from(&query);
            cx.emit(QueryChanged(query));
        }
    }

    /// The chats `rows` that match, then the messages, while searching
    /// with message results at hand.
    pub(super) fn searching(
        &self,
        rows: Vec<ChatRow>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let theme = *cx.theme();
        let results = self
            .message_results
            .clone()
            .filter(|_| !self.query.is_empty())?;
        let shown = u8::try_from(rows.len())
            .unwrap_or(u8::MAX)
            .min(MOST_CHAT_ROWS);
        let chats = (!rows.is_empty()).then(|| {
            div()
                .flex_none()
                .flex()
                .flex_col()
                .child(heading("Chats", theme))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .h(px(ROW_HEIGHT * f32::from(shown)))
                        .child(Self::chats(rows, &self.search_scroll, cx)),
                )
        });
        Some(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .children(chats)
                .child(div().flex_1().min_h_0().child(results))
                .into_any_element(),
        )
    }
}

fn heading(label: &'static str, theme: Theme) -> Div {
    div()
        .px_5()
        .pt_2()
        .pb_1()
        .text_size(px(13.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(theme.accent))
        .child(label)
}
