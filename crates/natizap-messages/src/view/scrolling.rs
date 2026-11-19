//! Where the conversation is scrolled to: kept on the same message when
//! the chat's messages are read again, followed to the end only when it
//! sat there, and a button back to the latest messages once scrolled up.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{Context, Div, ListOffset, ListState, div};
use natizap_ui::{Button, ButtonSize, ButtonVariant, Icon};

use super::ConversationView;
use crate::items::Item;

impl ConversationView {
    /// Hears the list scroll, to show the way back to the latest messages
    /// only while it is away from them.
    pub(super) fn watch_scroll(messages: &ListState, cx: &mut Context<Self>) {
        let view = cx.weak_entity();
        // The list is still busy scrolling when it calls this: what it says
        // in the event is all that may be read, not the list itself.
        messages.set_scroll_handler(move |event, _, cx| {
            let away = event.visible_range.end < event.count;
            let _ = view.update(cx, |view, cx| {
                if view.away_from_end != away {
                    view.away_from_end = away;
                    cx.notify();
                }
            });
        });
    }

    /// Shows `items`, the open chat's messages read again. In the same chat
    /// scrolled up, the message at the top stays where it was; at the end,
    /// or in another chat, the list starts at the latest message.
    pub(super) fn refill(&mut self, items: Arc<[Item]>, same_chat: bool) {
        let kept = (same_chat && self.messages.is_scrolled_to_end() == Some(false))
            .then(|| self.top_message())
            .flatten();
        self.messages.reset(items.len());
        if let Some((id, offset_in_item)) = kept
            && let Some(item_ix) = position(&items, &id)
        {
            self.messages.scroll_to(ListOffset {
                item_ix,
                offset_in_item,
            });
        } else {
            self.away_from_end = false;
        }
        self.shown_items = items;
    }

    /// The message at the top of the view, and how far into it the view
    /// starts; the first message below a day's heading when one is there.
    fn top_message(&self) -> Option<(gpui::SharedString, gpui::Pixels)> {
        let top = self.messages.logical_scroll_top();
        self.shown_items
            .iter()
            .enumerate()
            .skip(top.item_ix)
            .find_map(|(index, item)| match item {
                Item::Bubble(bubble) => Some((
                    bubble.id.clone(),
                    if index == top.item_ix {
                        top.offset_in_item
                    } else {
                        gpui::Pixels::ZERO
                    },
                )),
                Item::Day(_) => None,
            })
    }

    /// The round button back to the latest messages, while scrolled up.
    pub(super) fn latest_button(&self, cx: &mut Context<Self>) -> Option<Div> {
        self.away_from_end.then(|| {
            div().absolute().bottom_4().right_6().child(
                Button::new("to-latest")
                    .icon(Icon::ArrowDown)
                    .variant(ButtonVariant::Secondary)
                    .size(ButtonSize::Large)
                    .round()
                    .tooltip("Latest messages")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.messages.scroll_to_end();
                        this.away_from_end = false;
                        cx.notify();
                    })),
            )
        })
    }
}

fn position(items: &[Item], id: &str) -> Option<usize> {
    items
        .iter()
        .position(|item| matches!(item, Item::Bubble(bubble) if bubble.id.as_ref() == id))
}
