use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, Entity, FocusHandle, Focusable as _, FontWeight, ListAlignment,
    ListState, SharedString, Window, div, list, px, relative, rgb, svg,
};
use natizap_ui::theme::{
    ALERT, HONEY, HONEY_INK, LINE, MUTED, OUTGOING, PANE, PANEL, RAISED, TEXT,
};
use natizap_ui::{Icon, Submitted, TextInput, avatar};

use crate::items::{Body, Bubble, Item};
use crate::log::MessageLog;
use crate::message::Delivery;

/// The conversation pane: header, messages and composer for the open chat,
/// or a hint to pick one.
#[derive(Debug)]
pub struct ConversationView {
    log: Entity<MessageLog>,
    composer: Entity<TextInput>,
    messages: ListState,
    shown_chat: Option<SharedString>,
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
        }
    }

    /// Sends what the composer holds, if anything, and empties it.
    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).text().trim().to_owned();
        if text.is_empty() {
            return;
        }
        self.log.update(cx, |log, cx| log.send(text, cx));
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
            // A newly opened chat is where the user is about to type.
            let focus: FocusHandle = self.composer.focus_handle(cx);
            window.focus(&focus, cx);
        }
        self.messages.reset(count);
        cx.notify();
    }
}

impl Render for ConversationView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let log = self.log.read(cx);
        let Some(chat) = log.open_chat() else {
            return placeholder().into_any_element();
        };
        let items = log.items().clone();
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
                                div()
                                    .truncate()
                                    .text_size(px(15.5))
                                    .font_weight(FontWeight::SEMIBOLD)
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
                        .map_or_else(|| div().into_any_element(), render_item)
                })
                .flex_1()
                .px(px(56.0))
                .py_3(),
            )
            .child(composer(&self.composer, cx))
            .into_any_element()
    }
}

fn render_item(item: &Item) -> AnyElement {
    match item {
        Item::Day(label) => div()
            .flex()
            .justify_center()
            .pt_4()
            .pb_2()
            .child(
                div()
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .bg(rgb(PANEL))
                    .text_size(px(12.0))
                    .text_color(rgb(MUTED))
                    .child(label.clone()),
            )
            .into_any_element(),
        Item::Bubble(bubble) => render_bubble(bubble),
    }
}

fn render_bubble(bubble: &Bubble) -> AnyElement {
    let meta = div()
        .flex()
        .justify_end()
        .gap_1p5()
        .text_size(px(11.0))
        .text_color(rgb(MUTED))
        .when(
            matches!(bubble.body, Body::Content { edited: true, .. }),
            |meta| meta.child("edited"),
        )
        .child(bubble.time.clone())
        .map(|meta| match (bubble.from_me, bubble.delivery) {
            (true, Delivery::Sending) => meta.child("· sending"),
            (true, Delivery::Failed) => {
                meta.child(div().text_color(rgb(ALERT)).child("· not sent"))
            }
            _ => meta,
        });

    let body = match &bubble.body {
        Body::Deleted => div()
            .italic()
            .text_color(rgb(MUTED))
            .child("This message was deleted"),
        Body::Content { media, text, .. } => div()
            .flex()
            .flex_col()
            .gap_0p5()
            .children(media.map(|media| {
                div()
                    .text_size(px(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(MUTED))
                    .child(media)
            }))
            .children(text.clone()),
    };

    let shape = div()
        .max_w(relative(0.72))
        .flex()
        .flex_col()
        .gap_0p5()
        .px(px(10.0))
        .py(px(6.0))
        .rounded(px(14.0))
        .bg(rgb(if bubble.from_me { OUTGOING } else { RAISED }))
        .text_size(px(14.5))
        .line_height(relative(1.4))
        .map(|shape| match (bubble.starts_burst, bubble.from_me) {
            // The corner nearest the sender stays sharp on a burst's first
            // bubble, pointing at who said it.
            (true, true) => shape.rounded_tr(px(4.0)),
            (true, false) => shape.rounded_tl(px(4.0)),
            _ => shape,
        })
        .children(bubble.author.clone().map(|(name, color)| {
            div()
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(color))
                .child(name)
        }))
        // Time sits beside short text and drops below text that fills the
        // bubble, as in WhatsApp.
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_end()
                .gap_x_2()
                .child(body)
                .child(meta.ml_auto().pb(px(1.0))),
        );

    div()
        .id(bubble.id.clone())
        .w_full()
        .flex()
        .pt(px(if bubble.starts_burst { 8.0 } else { 2.0 }))
        .when(bubble.from_me, Styled::justify_end)
        .child(shape)
        .into_any_element()
}

fn composer(input: &Entity<TextInput>, cx: &mut Context<ConversationView>) -> Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap_3()
        .px_4()
        .py_3()
        .bg(rgb(PANEL))
        .border_t_1()
        .border_color(rgb(LINE))
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
                .child(input.clone()),
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
        )
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
