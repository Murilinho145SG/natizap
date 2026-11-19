use gpui::prelude::*;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use gpui::{
    AnyView, ClipboardItem, Context, Div, Entity, EventEmitter, ExternalPaths, FocusHandle,
    Focusable as _, FontWeight, KeyDownEvent, ListAlignment, ListState, Pixels, Point,
    SharedString, Window, anchored, deferred, div, px, rgb, svg,
};
use natizap_playback::Recorder;
use natizap_profiles::ProfilePictures;
use natizap_protocol::{ChatId, ContentKind, Reply};
use natizap_ui::theme::{ActiveTheme as _, Theme};
use natizap_ui::{FilesPasted, Icon, Submitted, TextInput};

use crate::authors::Tint;

use crate::carousel::Carousel;
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
mod carousel;
mod composer;
mod drop;
mod find;
mod going;
mod header;
mod list;
mod mentions;
mod message_menu;
mod recorder;
mod saving;
mod scrolling;
mod stickers;
mod tray;
mod typing;
mod viewer;
mod voice_notes;
mod votes;

use arrivals::Arrivals;
pub use attach::ContactsRequested;
pub use carousel::PlayRequested;
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
    /// The poll bubble whose votes show, by id.
    votes_shown: Option<SharedString>,
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
    /// The sticker shown large.
    viewing: Option<MediaView>,
    /// The photo or video open over the window, among the chat's others.
    carousel: Option<Carousel>,
    /// The search in the open chat, while its bar shows.
    find: Option<find::Find>,
    /// The messages the list shows, to keep its place when they change.
    shown_items: Arc<[Item]>,
    /// The list is scrolled up, away from the latest messages.
    away_from_end: bool,
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
    /// Mentions being written in the composer.
    mentions: mentions::Mentions,
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

/// The user asked to call the person the open chat is with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallRequested {
    /// Who to call.
    pub peer: ChatId,
    /// With the camera on.
    pub video: bool,
}

impl EventEmitter<CallRequested> for ConversationView {}

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
    color: Tint,
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
        let messages = ListState::new(0, ListAlignment::Bottom, px(600.0));
        Self::watch_scroll(&messages, cx);
        let composer = cx.new(|cx| TextInput::new("Message", cx).wrapping());
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
        // The send button turns into a microphone when the field empties,
        // and an `@` offers the group's members.
        cx.observe(&composer, |this, _, cx| {
            this.follow_mention(cx);
            cx.notify();
        })
        .detach();
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
            this.settle_carousel(cx);
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
            messages,
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
            votes_shown: None,
            selection: None,
            hovered_link: None,
            arrivals: Arrivals::default(),
            going_to: None,
            flash: None,
            viewing: None,
            carousel: None,
            find: None,
            shown_items: Arc::from([]),
            away_from_end: false,
            chat_menu: None,
            recorder: None,
            sticker_picker: None,
            favorite_stickers: HashSet::new(),
            sending: None,
            browsing: None,
            refocus: false,
            mentions: mentions::Mentions::default(),
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
        let ids = items.iter().filter_map(|item| match item {
            Item::Bubble(bubble) => Some(&bubble.id),
            Item::Day(_) => None,
        });
        if open == self.shown_chat {
            self.arrivals.saw(ids, Instant::now());
        } else {
            self.arrivals.reset(ids);
        }
        let same_chat = open == self.shown_chat;
        if !same_chat {
            self.shown_chat = open;
            self.replying = None;
            self.reacting = None;
            self.tray.close();
            // The files and the media window were for the chat left behind.
            self.sending = None;
            self.browsing = None;
            self.find = None;
            self.voices.stop();
            // A newly opened chat is where the user is about to type.
            let focus: FocusHandle = self.composer.focus_handle(cx);
            window.focus(&focus, cx);
        }
        self.refill(items, same_chat);
        self.keep_going(cx);
        cx.notify();
    }
}

impl ConversationView {
    /// Makes the next message a reply to `bubble`'s.
    pub(crate) fn reply_to(&mut self, bubble: &Bubble, cx: &mut Context<Self>) {
        let reply = bubble.reply.clone();
        let (author, color) = match (&reply.sender, &bubble.sender) {
            (None, _) => ("You".into(), Tint::Accent),
            (Some(_), Some(group_sender)) => (group_sender.name.clone(), group_sender.color),
            (Some(sender), None) => {
                let title = self.log.read(cx).open_chat().map(|chat| chat.title.clone());
                (title.unwrap_or_default(), Tint::person(sender.as_str()))
            }
        };
        self.replying = Some(Replying {
            reply,
            author,
            color,
        });
        cx.notify();
    }

    /// Opens an attachment, downloading it first: videos in the carousel,
    /// anything else as the system or its bubble plays it.
    pub(crate) fn open_media(&mut self, media: &MediaView, cx: &mut Context<Self>) {
        if media.fetch.kind == ContentKind::Video {
            self.view_media(media, cx);
            return;
        }
        let (fetch, file) = (media.fetch.clone(), media.file.clone());
        self.log
            .update(cx, |log, cx| log.open_media(fetch, file.as_deref(), cx));
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
        let theme = *cx.theme();
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
                            .map(|tray| tray::frame(tray, theme))
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
        if self.carousel.is_some() {
            self.close_carousel(cx);
        } else if self.viewing.take().is_some()
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
    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.key == "f" && keystroke.modifiers.secondary(cx) {
            self.open_find(window, cx);
            cx.stop_propagation();
        } else if keystroke.key == "escape" {
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
        let theme = *cx.theme();
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
            return placeholder(theme).into_any_element();
        };
        let pinned = log.pinned().cloned();
        let messages = self.message_list(&chat.id, window, cx);

        div()
            .size_full()
            .relative()
            .group("conversation")
            .flex()
            .flex_col()
            .bg(rgb(theme.pane))
            .on_drop(cx.listener(|this, dropped: &ExternalPaths, window, cx| {
                this.stage(dropped.paths().to_vec(), false, window, cx);
            }))
            .on_key_down(cx.listener(Self::key_down))
            .child(header(&chat, picture, members, presence, cx))
            .children(self.find_bar(cx))
            .when_some(self.sending.clone(), |pane, preview| {
                pane.child(div().flex_1().min_h_0().child(preview))
            })
            .when(self.sending.is_none(), |pane| {
                pane.children(pinned.map(|pinned| self.pinned_bar(pinned, cx)))
                    .child(messages)
                    .children(self.typing_note(cx))
                    .child(self.composer(window, cx))
            })
            // Only while a drag is on: the zone animates, and an animation
            // redraws every frame.
            .when(cx.has_active_drag(), |pane| pane.child(drop_zone(theme)))
            .children(self.message_popup(cx))
            .children(self.reaction_picker(cx))
            .children(self.attach_menu(cx))
            .children(self.poll_dialog(window, cx))
            .children(self.votes_dialog(window, cx))
            .children(self.browsing.clone())
            .children(self.picture_viewer(window, cx))
            .children(self.carousel_view(window, cx))
            .children(self.chat_menu(cx))
            .into_any_element()
    }
}

fn placeholder(theme: Theme) -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_3()
        .bg(rgb(theme.pane))
        .child(
            svg()
                .path(Icon::Chats.path())
                .size(px(56.0))
                .text_color(rgb(theme.line)),
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
                .text_color(rgb(theme.muted))
                .child(
                    svg()
                        .path(Icon::Lock.path())
                        .size(px(13.0))
                        .text_color(rgb(theme.muted)),
                )
                .child("End-to-end encrypted"),
        )
}
