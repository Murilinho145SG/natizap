use gpui::prelude::*;
use std::time::Duration;

use gpui::{
    Context, Div, Entity, FocusHandle, Focusable as _, FontWeight, KeyDownEvent, ListAlignment,
    ListState, MouseButton, Pixels, Point, SharedString, Window, anchored, deferred, div, list, px,
    rgb, svg,
};
use natizap_playback::Speed;
use natizap_profiles::ProfilePictures;
use natizap_protocol::Reply;
use natizap_ui::theme::{ACCENT, LINE, MUTED, PANE, PANEL};
use natizap_ui::{Icon, Submitted, TextInput, avatar, name_color};

use crate::bubble::render_item;
use crate::emoji::{EmojiPicked, EmojiPicker};
use crate::items::{Bubble, MediaView};
use crate::log::{MessageLog, SoundReady};
use crate::voice::Voices;

mod composer;
mod message_menu;

use message_menu::MessagePopup;

/// How often a playing note's bubble is redrawn: often enough for the
/// waveform and clock, far less than every frame.
const VOICE_TICK: Duration = Duration::from_millis(50);

/// The conversation pane: header, messages and composer for the open chat,
/// or a hint to pick one.
#[derive(Debug)]
pub struct ConversationView {
    log: Entity<MessageLog>,
    pictures: Entity<ProfilePictures>,
    composer: Entity<TextInput>,
    messages: ListState,
    shown_chat: Option<SharedString>,
    replying: Option<Replying>,
    /// The bubble whose quick reactions are open.
    reacting: Option<SharedString>,
    picking_emoji: bool,
    emoji_picker: Entity<EmojiPicker>,
    voices: Voices,
    /// Whether a task is redrawing the playing note.
    ticking: bool,
    /// A message's right-click menu or delete question, when open.
    message_popup: Option<MessagePopup>,
    /// The message being reacted to from the full picker, and where.
    reaction_picker: Option<(SharedString, Point<Pixels>)>,
    reaction_emojis: Entity<EmojiPicker>,
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
    pub fn new(
        log: Entity<MessageLog>,
        pictures: Entity<ProfilePictures>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&pictures, |_, _, cx| cx.notify()).detach();
        let composer = cx.new(|cx| TextInput::new("Message", cx));
        let emoji_picker = cx.new(|_| EmojiPicker::new());
        let reaction_emojis = cx.new(|_| EmojiPicker::new());
        cx.subscribe(&reaction_emojis, |this, _, EmojiPicked(emoji), cx| {
            let Some((id, _)) = this.reaction_picker.take() else {
                return;
            };
            if let Some(bubble) = this.bubble(&id, cx) {
                this.react(&bubble, emoji, cx);
            }
            cx.notify();
        })
        .detach();
        cx.subscribe_in(
            &emoji_picker,
            window,
            |this, _, EmojiPicked(emoji), window, cx| {
                // Keep typing where the emoji went.
                this.composer.update(cx, |input, cx| {
                    input.insert(emoji, window, cx);
                    window.focus(&input.focus_handle(cx), cx);
                });
            },
        )
        .detach();
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
            pictures,
            composer,
            messages: ListState::new(0, ListAlignment::Bottom, px(600.0)),
            shown_chat: None,
            replying: None,
            reacting: None,
            picking_emoji: false,
            emoji_picker,
            voices: Voices::default(),
            ticking: false,
            message_popup: None,
            reaction_picker: None,
            reaction_emojis,
        }
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
        let picker_was_open = self.reaction_picker.take().is_some();
        if self.reacting.is_some() || self.picking_emoji || menu_was_open || picker_was_open {
            self.reacting = None;
            self.picking_emoji = false;
            cx.notify();
        }
    }

    /// Opens the full emoji picker at `position` to react to message `id`.
    pub(crate) fn open_reaction_picker(
        &mut self,
        id: SharedString,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.close_popups(cx);
        self.reaction_picker = Some((id, position));
        cx.notify();
    }

    fn reaction_picker(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let (_, position) = self.reaction_picker.as_ref()?;
        let view = cx.weak_entity();
        Some(
            deferred(
                anchored()
                    .position(*position)
                    .snap_to_window_with_margin(px(8.0))
                    .child(
                        div()
                            .occlude()
                            .on_mouse_down_out(move |_, _, cx| {
                                let _ = view.update(cx, ConversationView::close_popups);
                            })
                            .child(self.reaction_emojis.clone()),
                    ),
            )
            .with_priority(1),
        )
    }

    /// Escape backs out of whatever is open, innermost first: popups, then
    /// the reply being composed.
    fn escape(&mut self, cx: &mut Context<Self>) {
        if self.message_popup.is_some() {
            self.close_message_popup(cx);
        } else if self.voices.close_speed_menu() {
            cx.notify();
        } else if self.reacting.is_some() || self.picking_emoji || self.reaction_picker.is_some() {
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
}

impl Render for ConversationView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.log.read(cx).open_chat().map(|chat| chat.id.clone());
        let picture = open.and_then(|chat| {
            self.pictures
                .update(cx, |pictures, cx| pictures.picture(&chat, cx))
        });
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
                    .child(avatar(chat.id.as_str(), &chat.title, picture, px(38.0)))
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
            .children(self.message_popup(cx))
            .children(self.reaction_picker(cx))
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
