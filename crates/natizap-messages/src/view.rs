use gpui::prelude::*;
use std::time::Duration;

use gpui::{
    Context, Div, Entity, FocusHandle, Focusable as _, FontWeight, KeyDownEvent, ListAlignment,
    ListState, MouseButton, SharedString, Stateful, Window, deferred, div, list, px, relative, rgb,
    svg,
};
use natizap_playback::Speed;
use natizap_protocol::{ContentKind, Reply};
use natizap_ui::theme::{ACCENT, LINE, MUTED, PANE, PANEL, RAISED, TEXT, glass};
use natizap_ui::{
    Button, ButtonSize, ButtonVariant, Icon, Submitted, TextInput, avatar, name_color,
};

use crate::bubble::{quote_block, render_item};
use crate::emoji::picker;
use crate::items::{Bubble, MediaView};
use crate::log::{MessageLog, SoundReady};
use crate::voice::Voices;

/// How often a playing note's bubble is redrawn: often enough for the
/// waveform and clock, far less than every frame.
const VOICE_TICK: Duration = Duration::from_millis(50);

/// The conversation pane: header, messages and composer for the open chat,
/// or a hint to pick one.
#[derive(Debug)]
pub struct ConversationView {
    log: Entity<MessageLog>,
    composer: Entity<TextInput>,
    messages: ListState,
    shown_chat: Option<SharedString>,
    replying: Option<Replying>,
    /// The bubble whose quick reactions are open.
    reacting: Option<SharedString>,
    picking_emoji: bool,
    voices: Voices,
    /// Whether a task is redrawing the playing note.
    ticking: bool,
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
        cx.subscribe(&log, |this, _, ready: &SoundReady, cx| {
            this.voices.downloaded(&ready.message_id, &ready.file);
            this.keep_ticking(cx);
        })
        .detach();
        Self {
            log,
            composer,
            messages: ListState::new(0, ListAlignment::Bottom, px(600.0)),
            shown_chat: None,
            replying: None,
            reacting: None,
            picking_emoji: false,
            voices: Voices::default(),
            ticking: false,
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
            self.reacting = None;
            self.picking_emoji = false;
            self.voices.stop();
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
            (None, _) => ("You".into(), ACCENT),
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

    /// Opens an attachment, downloading it first.
    pub(crate) fn open_media(&mut self, media: &MediaView, cx: &mut Context<Self>) {
        let (fetch, file) = (media.fetch.clone(), media.file.clone());
        self.log
            .update(cx, |log, cx| log.open_media(fetch, file.as_deref(), cx));
    }

    /// Plays or pauses a voice note or audio file in its bubble.
    pub(crate) fn toggle_voice(&mut self, media: &MediaView, cx: &mut Context<Self>) {
        let id = SharedString::from(media.fetch.message_id.clone());
        if self.voices.toggle(&id, media.file.as_deref()) {
            let fetch = media.fetch.clone();
            self.log
                .update(cx, |log, cx| log.open_media(fetch, None, cx));
        }
        self.keep_ticking(cx);
    }

    /// Jumps to `fraction` of the playing note.
    pub(crate) fn seek_voice(&mut self, message_id: &str, fraction: f32, cx: &mut Context<Self>) {
        self.voices.seek(message_id, fraction);
        cx.notify();
    }

    /// Plays voice notes at `speed` from now on.
    pub(crate) fn set_voice_speed(&mut self, speed: Speed, cx: &mut Context<Self>) {
        self.voices.set_speed(speed);
        cx.notify();
    }

    /// Opens or closes the speed menu of `message_id`'s bubble.
    pub(crate) fn toggle_speed_menu(&mut self, message_id: &SharedString, cx: &mut Context<Self>) {
        self.voices.toggle_speed_menu(message_id);
        cx.notify();
    }

    /// Redraws the playing note until it pauses, stops or ends.
    fn keep_ticking(&mut self, cx: &mut Context<Self>) {
        cx.notify();
        if self.ticking || !self.voices.is_running() {
            return;
        }
        self.ticking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(VOICE_TICK).await;
                let running = this.update(cx, |view, cx| {
                    cx.notify();
                    view.ticking = view.voices.is_running();
                    view.ticking
                });
                if !matches!(running, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Closes the quick reactions, the emoji picker and the speed menu.
    pub(crate) fn close_popups(&mut self, cx: &mut Context<Self>) {
        let menu_was_open = self.voices.close_speed_menu();
        if self.reacting.is_some() || self.picking_emoji || menu_was_open {
            self.reacting = None;
            self.picking_emoji = false;
            cx.notify();
        }
    }

    /// Escape backs out of whatever is open, innermost first: popups, then
    /// the reply being composed.
    fn escape(&mut self, cx: &mut Context<Self>) {
        if self.voices.close_speed_menu() {
            cx.notify();
        } else if self.reacting.is_some() || self.picking_emoji {
            self.close_popups(cx);
        } else if self.replying.take().is_some() {
            cx.notify();
        }
    }

    /// Opens or closes the quick reactions of the bubble `id`.
    pub(crate) fn toggle_reactions(&mut self, id: &SharedString, cx: &mut Context<Self>) {
        self.reacting = if self.reacting.as_ref() == Some(id) {
            None
        } else {
            Some(id.clone())
        };
        cx.notify();
    }

    /// Reacts to `bubble` with `emoji`; reacting again with the same emoji
    /// takes the reaction back.
    pub(crate) fn react(&mut self, bubble: &Bubble, emoji: &str, cx: &mut Context<Self>) {
        let emoji = (bubble.my_reaction.as_deref() != Some(emoji)).then(|| emoji.to_owned());
        let (id, sender) = (bubble.reply.id.clone(), bubble.reply.sender.clone());
        self.log
            .update(cx, |log, cx| log.react(id, sender, emoji, cx));
        self.reacting = None;
        cx.notify();
    }

    fn reply_bar(&self, cx: &mut Context<Self>) -> Option<Div> {
        let replying = self.replying.as_ref()?;
        Some(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    quote_block(
                        replying.author.clone(),
                        replying.color,
                        replying.reply.text.clone().into(),
                        ContentKind::Text,
                        None,
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
                        // Acts on press, so nothing else can claim the click
                        // first.
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                cx.stop_propagation();
                                this.replying = None;
                                cx.notify();
                            }),
                        )
                        .child(
                            svg()
                                .path(Icon::Close.path())
                                .size(px(16.0))
                                .text_color(rgb(MUTED)),
                        ),
                ),
        )
    }

    fn composer(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("composer")
            .relative()
            .flex_none()
            .flex()
            .flex_col()
            .gap_2()
            .px_4()
            .py_3()
            .bg(rgb(PANEL))
            .border_t_1()
            .border_color(rgb(LINE))
            .when(self.picking_emoji, |composer| {
                let input = self.composer.downgrade();
                // Floats above the messages instead of growing the composer,
                // which pushed the composer out of a tall message list.
                composer.child(deferred(
                    div()
                        .absolute()
                        .bottom(relative(1.0))
                        .left_4()
                        .mb_2()
                        .w(px(388.0))
                        .occlude()
                        .child(picker(move |emoji, window, cx| {
                            // Keep typing where the emoji went.
                            let _ = input.update(cx, |input, cx| {
                                input.insert(emoji, window, cx);
                                window.focus(&input.focus_handle(cx), cx);
                            });
                        })),
                ))
            })
            .children(self.reply_bar(cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        Button::new("emoji")
                            .icon(Icon::Emoji)
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Large)
                            .round()
                            .selected(self.picking_emoji)
                            .tooltip("Emoji")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.picking_emoji = !this.picking_emoji;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px_4()
                            .py_2p5()
                            .rounded_full()
                            .bg(rgb(RAISED))
                            .map(glass)
                            .text_size(px(14.5))
                            .text_color(rgb(TEXT))
                            .child(self.composer.clone()),
                    )
                    .child(
                        Button::new("send")
                            .icon(Icon::Send)
                            .size(ButtonSize::Large)
                            .round()
                            .tooltip("Send")
                            .on_click(cx.listener(|this, _, window, cx| this.send(window, cx))),
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
        let reacting = self.reacting.clone();
        let voice = self.voices.ui();
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
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.escape(cx);
                }
            }))
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
                                    .text_size(px(16.0))
                                    // Medium on purpose: at semibold, symbols such as ♥ render
                                    // monochrome even with the emoji font as fallback.
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
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
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
                        list(self.messages.clone(), move |index, _, _| {
                            items.get(index).map_or_else(
                                || div().into_any_element(),
                                |item| render_item(item, &view, reacting.as_ref(), &voice),
                            )
                        })
                        .flex_1()
                        .px(px(56.0))
                        .py_3(),
                    ),
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
