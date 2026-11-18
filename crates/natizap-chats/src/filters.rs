//! What narrows the chat list: the search field, the filter chips, and the
//! archived chats in place of the rest.

use gpui::prelude::*;
use gpui::{App, Div, Entity, FontWeight, SharedString, div, px, rgb, svg};
use natizap_ui::theme::{ACCENT, ACCENT_INK, LINE, MUTED, RAISED, TEXT};
use natizap_ui::{Icon, TextInput};

use crate::list::ChatRow;

/// Which chats the list shows, as the chips above it choose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Filter {
    #[default]
    All,
    Unread,
    Groups,
}

impl Filter {
    const ALL: [Self; 3] = [Self::All, Self::Unread, Self::Groups];

    fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Unread => "Unread",
            Self::Groups => "Groups",
        }
    }

    fn keeps(self, row: &ChatRow) -> bool {
        match self {
            Self::All => true,
            Self::Unread => row.unread > 0 || row.marked_unread,
            Self::Groups => row.id.is_group(),
        }
    }
}

/// The rows of `rows` that `filter` keeps and that answer `query`.
pub(crate) fn narrowed(rows: &[ChatRow], filter: Filter, query: &str) -> Vec<ChatRow> {
    rows.iter()
        .filter(|row| filter.keeps(row) && row.matches(query))
        .cloned()
        .collect()
}

/// The rounded search field.
pub(crate) fn search_field(input: &Entity<TextInput>) -> Div {
    div()
        .mx_3()
        .px_3()
        .h(px(36.0))
        .flex()
        .items_center()
        .gap_2()
        .rounded_full()
        .bg(rgb(RAISED))
        .text_size(px(14.0))
        .child(
            svg()
                .path(Icon::Search.path())
                .size(px(15.0))
                .flex_none()
                .text_color(rgb(MUTED)),
        )
        .child(div().flex_1().min_w_0().child(input.clone()))
}

/// The filter chips, `chosen` lit; `unread` counts the chats to read.
pub(crate) fn chips(
    chosen: Filter,
    unread: usize,
    on_choose: impl Fn(Filter, &mut App) + Clone + 'static,
) -> Div {
    div()
        .px_3()
        .flex()
        .gap_1p5()
        .children(Filter::ALL.into_iter().map(|filter| {
            let on = filter == chosen;
            let label: SharedString = match filter {
                Filter::Unread if unread > 0 => format!("{} {unread}", filter.label()).into(),
                _ => filter.label().into(),
            };
            let on_choose = on_choose.clone();
            div()
                .id(filter.label())
                .px_3()
                .h(px(28.0))
                .flex()
                .items_center()
                .rounded_full()
                .border_1()
                .text_size(px(13.0))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .map(|chip| {
                    if on {
                        chip.bg(rgb(ACCENT))
                            .border_color(rgb(ACCENT))
                            .text_color(rgb(ACCENT_INK))
                    } else {
                        chip.border_color(rgb(LINE))
                            .text_color(rgb(MUTED))
                            .hover(|chip| chip.text_color(rgb(TEXT)))
                    }
                })
                .on_click(move |_, _, cx| on_choose(filter, cx))
                .child(label)
        }))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use natizap_protocol::ChatId;

    use super::*;

    fn row(id: &str, title: &str, unread: u32) -> ChatRow {
        ChatRow {
            id: ChatId::new(id),
            addresses: Arc::from([ChatId::new(id)]),
            title: title.to_owned().into(),
            preview: None,
            preview_kind: None,
            sent_at: None,
            unread,
            marked_unread: false,
            pinned: false,
            muted: false,
            verified: false,
            business: false,
            disappearing: None,
        }
    }

    #[test]
    fn narrows_by_chip_name_and_number() {
        let rows = [
            row("5511987654321@s.whatsapp.net", "Bruno", 0),
            row("120363@g.us", "Família", 2),
            row("5541988776655@s.whatsapp.net", "Padaria", 1),
        ];
        let titles = |filter, query| -> Vec<String> {
            narrowed(&rows, filter, query)
                .iter()
                .map(|row| row.title.to_string())
                .collect()
        };
        assert_eq!(titles(Filter::Unread, ""), ["Família", "Padaria"]);
        assert_eq!(titles(Filter::Groups, ""), ["Família"]);
        assert_eq!(titles(Filter::All, "FAMÍ"), ["Família"]);
        assert_eq!(titles(Filter::All, "11 98765"), ["Bruno"]);
        assert_eq!(titles(Filter::Unread, "bruno"), Vec::<String>::new());
    }
}
