use gpui::prelude::*;
use gpui::{
    Context, Div, Entity, FocusHandle, Focusable as _, FontWeight, ListAlignment, ListState,
    SharedString, Window, div, list, px, rgb, svg,
};
use natizap_protocol::Reply;
use natizap_ui::theme::{HONEY, HONEY_INK, LINE, MUTED, PANE, PANEL, RAISED, TEXT};
use natizap_ui::{Icon, Submitted, TextInput, avatar, name_color};

use crate::bubble::{quote_block, render_item};
use crate::items::Bubble;
use crate::log::MessageLog;

/// The conversation pane: header, messages and composer for the open chat,
/// or a hint to pick one.
#[derive(Debug)]
pub struct ConversationView {
    log: Entity<MessageLog>,
    composer: Entity<TextInput>,
    messages: ListState,
    shown_chat: Option<SharedString>,
    replying: Option<Replying>,
}

/// The message the next one will reply to, as the composer shows it.
#[derive(Debug, Clone)]
struct Replying {
    reply: Reply,
    author: SharedString,
    color: u32,
}

impl ConversationView {
    /// Creates a view that follows `log`'s open chat.
    pub fn new(log: Entity<MessageLog>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let composer = cx.new(|cx| TextInput::new("Message", cx));
        cx.subscribe_in(&composer, window, |this, _, _: &Submitted, window, cx| {
            this.send(window, cx);
        })
        .detach();
        cx.observe_in(&log, window, |this, log, window, cx| {
            this.follow(&log, window, cx);
        })
        .detach();
        Self {
            log,
            composer,
            messages: ListState::new(0, ListAlignment::Bottom, px(600.0)),
            shown_chat: None,
            replying: None,
        }
    }

    /// Sends what the composer holds, if anything, and empties it.
    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).text().trim().to_owned();
        if text.is_empty() {
            return;
        }
        let reply = self.replying.take().map(|replying| replying.reply);
        self.log.update(cx, |log, cx| log.send(text, reply, cx));
        self.composer
            .update(cx, |composer, cx| composer.clear(window, cx));
    }

    fn follow(&mut self, log: &Entity<MessageLog>, window: &mut Window, cx: &mut Context<Self>) {
        let (open, count) = {
            let log = log.read(cx);
            let open = log
                .open_chat()
                .map(|chat| SharedString::from(chat.id.as_str().to_owned()));
            (open, log.items().len())
        };
        if open != self.shown_chat {
            self.shown_chat = open;
            self.replying = None;
            // A newly opened chat is where the user is about to type.
            let focus: FocusHandle = self.composer.focus_handle(cx);
            window.focus(&focus, cx);
        }
        self.messages.reset(count);
        cx.notify();
    }
}

impl ConversationView {
    /// Makes the next message a reply to `bubble`'s.
    pub(crate) fn reply_to(&mut self, bubble: &Bubble, cx: &mut Context<Self>) {
        let reply = bubble.reply.clone();
        let (author, color) = match (&reply.sender, &bubble.sender) {
            (None, _) => ("You".into(), HONEY),
            (Some(_), Some(group_sender)) => group_sender.clone(),
            (Some(sender), None) => {
                let title = self.log.read(cx).open_chat().map(|chat| chat.title.clone());
                (title.unwrap_or_default(), name_color(sender.as_str()))
            }
        };
        self.replying = Some(Replying {
            reply,
            author,
            color,
        });
        cx.notify();
    }

    fn composer(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex_none()
            .flex()
            .flex_col()
            .gap_2()
            .px_4()
            .py_3()
            .bg(rgb(PANEL))
            .border_t_1()
            .border_color(rgb(LINE))
            .children(self.replying.as_ref().map(|replying| {
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        quote_block(
                            replying.author.clone(),
                            replying.color,
                            replying.reply.text.clone().into(),
                        )
                        .flex_1()
                        .min_w_0(),
                    )
                    .child(
                        div()
                            .id("cancel-reply")
                            .size(px(28.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(RAISED)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.replying = None;
                                cx.notify();
                            }))
                            .child(
                                svg()
                                    .path(Icon::Close.path())
                                    .size(px(16.0))
                                    .text_color(rgb(MUTED)),
                            ),
                    )
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px_4()
                            .py_2p5()
                            .rounded_full()
                            .bg(rgb(RAISED))
                            .text_size(px(14.5))
                            .text_color(rgb(TEXT))
                            .child(self.composer.clone()),
                    )
                    .child(
                        div()
                            .id("send")
                            .size(px(40.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(rgb(HONEY))
                            .cursor_pointer()
                            .hover(|style| style.opacity(0.9))
                            .on_click(cx.listener(|this, _, window, cx| this.send(window, cx)))
                            .child(
                                svg()
                                    .path(Icon::Send.path())
                                    .size(px(18.0))
                                    .text_color(rgb(HONEY_INK)),
                            ),
                    ),
            )
    }
}

impl Render for ConversationView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let log = self.log.read(cx);
        let Some(chat) = log.open_chat() else {
            return placeholder().into_any_element();
        };
        let items = log.items().clone();
        let view = cx.weak_entity();
        let subtitle = if chat.id.is_group() {
            Some("Group".to_owned())
        } else {
            chat.id
                .phone_number()
                .filter(|phone| *phone != chat.title.as_ref())
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(PANE))
            .child(
                div()
                    .h(px(60.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_4()
                    .bg(rgb(PANEL))
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .child(avatar(chat.id.as_str(), &chat.title, px(38.0)))
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                // Medium, like the chat list: at heavier weights
                                // the system's font fallback picks monochrome
                                // emoji, and GPUI cannot be told to prefer an
                                // emoji font (it rejects fallbacks without
                                // Latin glyphs).
                                div()
                                    .truncate()
                                    .text_size(px(16.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(chat.title.clone()),
                            )
                            .children(subtitle.map(|subtitle| {
                                div()
                                    .text_size(px(12.5))
                                    .text_color(rgb(MUTED))
                                    .child(subtitle)
                            })),
                    ),
            )
            .child(
                list(self.messages.clone(), move |index, _, _| {
                    items
                        .get(index)
                        .map_or_else(|| div().into_any_element(), |item| render_item(item, &view))
                })
                .flex_1()
                .px(px(56.0))
                .py_3(),
            )
            .child(self.composer(cx))
            .into_any_element()
    }
}

fn placeholder() -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_3()
        .bg(rgb(PANE))
        .child(
            svg()
                .path(Icon::Chats.path())
                .size(px(56.0))
                .text_color(rgb(LINE)),
        )
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .child("Pick a chat to read it here"),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_1p5()
                .text_size(px(12.5))
                .text_color(rgb(MUTED))
                .child(
                    svg()
                        .path(Icon::Lock.path())
                        .size(px(13.0))
                        .text_color(rgb(MUTED)),
                )
                .child("End-to-end encrypted"),
        )
}
