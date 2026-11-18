use gpui::prelude::*;
use std::collections::HashSet;
use std::time::{Duration, Instant};

use gpui::{
    AnyView, ClipboardItem, Context, Div, Entity, EventEmitter, ExternalPaths, FocusHandle,
    Focusable as _, FontWeight, KeyDownEvent, ListAlignment, ListState, Pixels, Point,
    SharedString, Window, anchored, deferred, div, px, rgb, svg,
};
use natizap_playback::{Recorder, Speed};
use natizap_profiles::ProfilePictures;
use natizap_protocol::{ChatId, Reply};
use natizap_ui::theme::{ACCENT, LINE, MUTED, PANE};
use natizap_ui::{FilesPasted, Icon, Submitted, TextInput, name_color};

use crate::emoji::{EmojiPicked, EmojiPicker};
use crate::items::{Bubble, Item, MediaView};
use crate::log::{MessageLog, SoundReady};
use crate::media_browser::MediaBrowser;
use crate::poll::Ballot;
use crate::poll_composer::PollComposer;
use crate::rich_text::{HoveredLink, Selection};
use crate::send_preview::SendPreview;
use crate::voice::Voices;

mod arrivals;
mod attach;
mod composer;
mod drop;
mod going;
mod header;
mod list;
mod message_menu;
mod recorder;
mod stickers;
mod tray;
mod viewer;

use arrivals::Arrivals;
pub use attach::ContactsRequested;
use drop::drop_zone;
use going::GoingTo;
use header::header;
pub use header::{ChatClosed, MuteRequested};
use message_menu::MessagePopup;
pub use stickers::StickerFavoriteToggled;
use tray::Tray;

/// Width of the column a community's announcements read in.
const CENTERED_WIDTH: f32 = 720.0;

/// How far a message that just arrived rises into place, in pixels.
const ARRIVAL_RISE: f32 = 14.0;

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
    /// The emoji and stickers tray, when open, and the tab it shows.
    tray: Tray,
    emoji_picker: Entity<EmojiPicker>,
    voices: Voices,
    /// Whether a task is redrawing the playing note.
    ticking: bool,
    /// A message's right-click menu or delete question, when open.
    message_popup: Option<MessagePopup>,
    /// The message being reacted to from the full picker, and where.
    reaction_picker: Option<(SharedString, Point<Pixels>)>,
    reaction_emojis: Entity<EmojiPicker>,
    /// Where the plus button's menu is open.
    attach_at: Option<Point<Pixels>>,
    /// A poll being written.
    poll_draft: Option<Entity<PollComposer>>,
    /// Text selected in a message.
    selection: Option<Selection>,
    /// The link the pointer rests on.
    hovered_link: Option<HoveredLink>,
    /// Messages that just arrived, to animate in.
    arrivals: Arrivals,
    /// A message being gone to, until it shows.
    going_to: Option<GoingTo>,
    /// The message just gone to, lit up for a moment, and when.
    flash: Option<(SharedString, Instant)>,
    /// The photo shown large.
    viewing: Option<MediaView>,
    /// Where the header's menu is open.
    chat_menu: Option<Point<Pixels>>,
    /// A voice note being recorded.
    recorder: Option<Recorder>,
    /// The favorite stickers to pick from, as the app hands them over.
    sticker_picker: Option<AnyView>,
    /// The hashes of the favorite stickers.
    favorite_stickers: HashSet<String>,
    /// Files about to be sent, shown in place of the messages.
    sending: Option<Entity<SendPreview>>,
    /// The window of a chat's media, links and docs.
    browsing: Option<Entity<MediaBrowser>>,
    /// The composer takes the keyboard back at the next draw, after a
    /// window that had it closed.
    refocus: bool,
}

/// The user clicked the conversation's header to see the chat's profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileRequested {
    /// Whose profile: the chat's phone-number address when it has one.
    pub chat: ChatId,
    /// The chat's name, shown until the profile loads.
    pub title: SharedString,
    /// A business WhatsApp vouches for.
    pub verified: bool,
}

impl EventEmitter<ProfileRequested> for ConversationView {}

/// The user clicked a phone number in a message, to chat with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatRequested {
    /// The number's phone-number address.
    pub phone: ChatId,
}

impl EventEmitter<ChatRequested> for ConversationView {}

impl EventEmitter<ContactsRequested> for ConversationView {}

impl EventEmitter<ChatClosed> for ConversationView {}

impl EventEmitter<MuteRequested> for ConversationView {}

impl EventEmitter<StickerFavoriteToggled> for ConversationView {}

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
        // The send button turns into a microphone when the field empties.
        cx.observe(&composer, |_, _, cx| cx.notify()).detach();
        cx.subscribe_in(&composer, window, |this, _, _: &Submitted, window, cx| {
            this.send(window, cx);
        })
        .detach();
        cx.subscribe_in(
            &composer,
            window,
            |_, _, pasted: &FilesPasted, window, cx| {
                Self::stage_pasted(pasted, window, cx);
            },
        )
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
            tray: Tray::default(),
            emoji_picker,
            voices: Voices::default(),
            ticking: false,
            message_popup: None,
            reaction_picker: None,
            attach_at: None,
            poll_draft: None,
            selection: None,
            hovered_link: None,
            arrivals: Arrivals::default(),
            going_to: None,
            flash: None,
            viewing: None,
            chat_menu: None,
            recorder: None,
            sticker_picker: None,
            favorite_stickers: HashSet::new(),
            sending: None,
            browsing: None,
            refocus: false,
            reaction_emojis,
        }
    }

    fn follow(&mut self, log: &Entity<MessageLog>, window: &mut Window, cx: &mut Context<Self>) {
        let (open, items) = {
            let log = log.read(cx);
            let open = log
                .open_chat()
                .map(|chat| SharedString::from(chat.id.as_str().to_owned()));
            (open, log.items().clone())
        };
        let count = items.len();
        let ids = items.iter().filter_map(|item| match item {
            Item::Bubble(bubble) => Some(&bubble.id),
            Item::Day(_) => None,
        });
        if open == self.shown_chat {
            self.arrivals.saw(ids, Instant::now());
        } else {
            self.arrivals.reset(ids);
        }
        if open != self.shown_chat {
            self.shown_chat = open;
            self.replying = None;
            self.reacting = None;
            self.tray.close();
            // The files and the media window were for the chat left behind.
            self.sending = None;
            self.browsing = None;
            self.voices.stop();
            // A newly opened chat is where the user is about to type.
            let focus: FocusHandle = self.composer.focus_handle(cx);
            window.focus(&focus, cx);
        }
        self.messages.reset(count);
        self.keep_going(cx);
        cx.notify();
    }
}

impl ConversationView {
    /// Makes the next message a reply to `bubble`'s.
    pub(crate) fn reply_to(&mut self, bubble: &Bubble, cx: &mut Context<Self>) {
        let reply = bubble.reply.clone();
        let (author, color) = match (&reply.sender, &bubble.sender) {
            (None, _) => ("You".into(), ACCENT),
            (Some(_), Some(group_sender)) => (group_sender.name.clone(), group_sender.color),
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
        if self.reacting.is_some() || self.tray.is_open() || menu_was_open || picker_was_open {
            self.reacting = None;
            self.tray.close();
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
                            .h(px(crate::emoji::HEIGHT))
                            .map(tray::frame)
                            .child(self.reaction_emojis.clone()),
                    ),
            )
            .with_priority(1),
        )
    }

    /// Backs out of whatever is open, innermost first: a message's menu or
    /// question, popups, then the reply being composed. Returns whether
    /// anything was open; escape and the back button both use it.
    pub fn step_back(&mut self, cx: &mut Context<Self>) -> bool {
        if self.viewing.take().is_some()
            || self.browsing.take().is_some()
            || self.sending.take().is_some()
        {
            cx.notify();
        } else if self.message_popup.is_some() {
            self.close_message_popup(cx);
        } else if self.voices.close_speed_menu() {
            cx.notify();
        } else if self.reacting.is_some() || self.tray.is_open() || self.reaction_picker.is_some() {
            self.close_popups(cx);
        } else if self.replying.take().is_some() {
            cx.notify();
        } else {
            return false;
        }
        true
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

    /// Votes for `options` on the poll `ballot` is for.
    pub(crate) fn vote(&mut self, ballot: &Ballot, options: Vec<String>, cx: &mut Context<Self>) {
        self.log.update(cx, |log, cx| log.vote(ballot, options, cx));
    }
}

impl ConversationView {
    /// The text selected in a message, if any.
    pub(crate) fn text_selection(&self) -> Option<&Selection> {
        self.selection.as_ref()
    }

    /// Escape steps back out of what is open; the copy shortcut copies the
    /// text selected in a message.
    fn key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.key == "escape" {
            self.step_back(cx);
        } else if keystroke.key == "c"
            && keystroke.modifiers.secondary(cx)
            && let Some(selected) = self.selection.as_ref().and_then(Selection::selected)
        {
            cx.write_to_clipboard(ClipboardItem::new_string(selected.to_owned()));
            cx.stop_propagation();
        }
    }

    /// Lights up `link`, or nothing, as the pointer moves over the text
    /// stretch `key`. Leaving a stretch clears only its own link, since the
    /// pointer may already rest on another stretch's.
    pub(crate) fn hover_link(
        &mut self,
        key: &SharedString,
        link: Option<HoveredLink>,
        cx: &mut Context<Self>,
    ) {
        let hovered = match link {
            Some(link) => Some(link),
            None if self
                .hovered_link
                .as_ref()
                .is_some_and(|hovered| hovered.is_in(key)) =>
            {
                None
            }
            None => return,
        };
        if self.hovered_link != hovered {
            self.hovered_link = hovered;
            cx.notify();
        }
    }

    /// Selects text in a message, or nothing.
    pub(crate) fn select_text(&mut self, selection: Option<Selection>, cx: &mut Context<Self>) {
        if self.selection != selection {
            self.selection = selection;
            cx.notify();
        }
    }
}

impl Render for ConversationView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.refocus) {
            window.focus(&self.composer.focus_handle(cx), cx);
        }
        let open = self.log.read(cx).open_chat().map(|chat| chat.id.clone());
        let picture = open.and_then(|chat| {
            self.pictures
                .update(cx, |pictures, cx| pictures.picture(&chat, cx))
        });
        let log = self.log.read(cx);
        let members = log.members_line(cx);
        let presence = log.presence_line();
        let Some(chat) = log.open_chat().cloned() else {
            return placeholder().into_any_element();
        };
        let pinned = log.pinned().cloned();
        let messages = self.message_list(&chat.id, window, cx);

        div()
            .size_full()
            .relative()
            .group("conversation")
            .flex()
            .flex_col()
            .bg(rgb(PANE))
            .on_drop(cx.listener(|this, dropped: &ExternalPaths, window, cx| {
                this.stage(dropped.paths().to_vec(), false, window, cx);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.key_down(event, cx)))
            .child(header(&chat, picture, members, presence, cx))
            .when_some(self.sending.clone(), |pane, preview| {
                pane.child(div().flex_1().min_h_0().child(preview))
            })
            .when(self.sending.is_none(), |pane| {
                pane.children(pinned.map(|pinned| self.pinned_bar(pinned, cx)))
                    .child(messages)
                    .child(self.composer(cx))
            })
            // Only while a drag is on: the zone animates, and an animation
            // redraws every frame.
            .when(cx.has_active_drag(), |pane| pane.child(drop_zone()))
            .children(self.message_popup(cx))
            .children(self.reaction_picker(cx))
            .children(self.attach_menu(cx))
            .children(self.poll_dialog(window, cx))
            .children(self.browsing.clone())
            .children(self.picture_viewer(window, cx))
            .children(self.chat_menu(cx))
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
