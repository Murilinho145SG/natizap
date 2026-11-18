//! The open chat's messages: a list drawn lazily, row by row, with the
//! messages that just arrived rising into place.

use std::collections::HashSet;
use std::time::Instant;

use gpui::prelude::*;
use gpui::{AnimationElement, Context, Div, MouseButton, SharedString, Window, div, list, px};
use natizap_protocol::ChatId;
use natizap_ui::motion::rise_in;
use natizap_ui::scrollbar;

use super::{ARRIVAL_RISE, CENTERED_WIDTH, ConversationView};
use crate::bubble::{LIT_FOR, render_item};
use crate::items::Item;
use crate::rich_text::TextFocus;

impl ConversationView {
    /// The messages of the open chat `chat`, which rise in as a whole when
    /// the chat opens.
    pub(super) fn message_list(
        &mut self,
        chat: &ChatId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnimationElement<Div> {
        let log = self.log.read(cx);
        let items = log.items().clone();
        let pictures = self.pictures.clone();
        // A community's announcements read as a column down the middle, as
        // on the phone.
        let centered = log
            .group()
            .is_some_and(|group| group.community_announcements);
        let view = cx.weak_entity();
        let reacting = self.reacting.clone();
        let voice = self.voices.ui();
        let now = Instant::now();
        // The message just gone to, while it is still lit up.
        let flash = self
            .flash
            .clone()
            .filter(|(_, at)| now.duration_since(*at) < LIT_FOR);
        let arrivals: HashSet<SharedString> = items
            .iter()
            .filter_map(|item| match item {
                Item::Bubble(bubble) if self.arrivals.is_arriving(&bubble.id, now) => {
                    Some(bubble.id.clone())
                }
                _ => None,
            })
            .collect();
        let focus = TextFocus {
            selection: self.selection.clone(),
            hovered_link: self.hovered_link.clone(),
        };

        let scrollbar = scrollbar("messages-scrollbar", &self.messages, window, cx);
        let messages: Div = div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .items_center()
            // A press anywhere drops the selected text; one on a
            // message's text then starts a new selection.
            .capture_any_mouse_down(cx.listener(|this, _, _, cx| {
                if this.selection.take().is_some() {
                    cx.notify();
                }
            }))
            // Clicking the conversation closes the emoji picker; the
            // quick reactions close themselves.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.picking_emoji {
                        this.picking_emoji = false;
                        cx.notify();
                    }
                }),
            )
            .child(
                list(self.messages.clone(), move |index, _, cx| {
                    let Some(item) = items.get(index) else {
                        return div().into_any_element();
                    };
                    // Only a burst's first message shows its sender.
                    let picture = match item {
                        Item::Bubble(bubble) => bubble.author.as_ref().and_then(|author| {
                            pictures.update(cx, |pictures, cx| pictures.picture(&author.id, cx))
                        }),
                        Item::Day(_) => None,
                    };
                    let lit = matches!(item, Item::Bubble(bubble)
                        if flash.as_ref().is_some_and(|(id, _)| *id == bubble.id));
                    let shown =
                        render_item(item, &view, reacting.as_ref(), &voice, &focus, picture, lit);
                    match item {
                        Item::Bubble(bubble) if arrivals.contains(&bubble.id) => rise_in(
                            SharedString::from(format!("arrive-{}", bubble.id)),
                            div().w_full().child(shown),
                            ARRIVAL_RISE,
                        )
                        .into_any_element(),
                        _ => shown,
                    }
                })
                .h_full()
                .w_full()
                .py_3()
                .when(centered, |list| list.max_w(px(CENTERED_WIDTH)).px_4())
                .when(!centered, |list| list.px(px(56.0))),
            )
            .child(scrollbar);
        rise_in(
            SharedString::from(format!("chat-{}", chat.as_str())),
            messages,
            CHAT_RISE,
        )
    }
}

/// How far a newly opened chat's messages rise into place, in pixels.
const CHAT_RISE: f32 = 10.0;
