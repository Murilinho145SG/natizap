//! What a right click on a chat offers without opening it, and the
//! questions its lasting actions ask first.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Pixels, Point, WeakEntity, Window};
use natizap_protocol::{ChatAction, ChatId};
use natizap_ui::{AlertDialog, ButtonVariant, Icon, Menu};

use super::ChatListView;
use crate::actions::MAX_PINNED;
use crate::list::ChatRow;

/// The chat a menu or question is about, by id, so it shows the chat as it
/// is now.
#[derive(Debug, Clone)]
pub(super) enum ChatPopup {
    /// The right-click menu, where the click happened.
    Menu(ChatId, Point<Pixels>),
    /// Asking before something that cannot be taken back.
    Asking(ChatId, Question),
    /// Saying why one more chat cannot be pinned.
    PinLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Question {
    Block,
    Clear,
    Delete,
}

/// The user cleared or deleted a chat from the list: the messages filed
/// under its addresses are to go too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatForgotten {
    /// Every address the chat's messages may be filed under.
    pub addresses: Arc<[ChatId]>,
    /// The chat itself went, starred messages and all; else only its
    /// messages did, starred ones aside.
    pub deleted: bool,
}

impl ChatListView {
    pub(super) fn open_menu(&mut self, chat: ChatId, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.popup = Some(ChatPopup::Menu(chat, at));
        cx.notify();
    }

    fn close_popup(&mut self, cx: &mut Context<Self>) {
        if self.popup.take().is_some() {
            cx.notify();
        }
    }

    /// The open menu or question, if any.
    pub(super) fn popup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let row = |chat: &ChatId, cx: &App| {
            let list = self.list.read(cx);
            list.rows()
                .iter()
                .chain(list.archived().iter())
                .find(|row| row.id == *chat)
                .cloned()
        };
        Some(match self.popup.as_ref()? {
            ChatPopup::Menu(chat, at) => self.menu(&row(chat, cx)?, cx).at(*at).into_any_element(),
            ChatPopup::Asking(chat, question) => {
                ask(*question, row(chat, cx)?, cx).into_any_element()
            }
            ChatPopup::PinLimit => pin_limit(cx).into_any_element(),
        })
    }

    /// Archive, mute, pin and mark read as on the phone, then blocking,
    /// clearing and deleting, which ask first.
    fn menu(&self, row: &ChatRow, cx: &mut Context<Self>) -> Menu {
        let can_pin = self.list.read(cx).can_pin();
        let unread = row.unread > 0 || row.marked_unread;
        let archived = self.in_archive;
        let view = cx.weak_entity();
        let act = |action: ChatAction| {
            let (view, row) = (view.clone(), row.clone());
            move |_: &mut Window, cx: &mut App| {
                let _ = view.update(cx, |this, cx| {
                    this.close_popup(cx);
                    this.list.read(cx).act(&row, action, cx);
                });
            }
        };

        let mut menu = Menu::new()
            .on_dismiss({
                let view = view.clone();
                move |_, cx| {
                    let _ = view.update(cx, ChatListView::close_popup);
                }
            })
            .action(
                Icon::Archive,
                if archived {
                    "Unarchive chat"
                } else {
                    "Archive chat"
                },
                act(ChatAction::Archive(!archived)),
            );
        let (muting, muted) = (view.clone(), row.muted);
        let chat = row.id.clone();
        menu = menu.action(
            Icon::Muted,
            if muted {
                "Unmute notifications"
            } else {
                "Mute notifications"
            },
            move |_, cx| {
                let _ = muting.update(cx, |this, cx| {
                    this.close_popup(cx);
                    this.list.read(cx).set_muted(&chat, !muted, cx);
                });
            },
        );
        // Archived chats cannot be pinned, as on the phone.
        if !archived {
            menu = if row.pinned {
                menu.action(Icon::Pin, "Unpin chat", act(ChatAction::Pin(false)))
            } else if can_pin {
                menu.action(Icon::Pin, "Pin chat", act(ChatAction::Pin(true)))
            } else {
                menu.action(Icon::Pin, "Pin chat", show(&view, ChatPopup::PinLimit))
            };
        }
        menu = if unread {
            menu.action(
                Icon::Checks,
                "Mark as read",
                act(ChatAction::MarkRead(true)),
            )
        } else {
            menu.action(
                Icon::Unread,
                "Mark as unread",
                act(ChatAction::MarkRead(false)),
            )
        };
        self.lasting(menu.separator(), row, cx)
    }

    /// Blocking, clearing and deleting, which ask first; unblocking does
    /// not.
    fn lasting(&self, mut menu: Menu, row: &ChatRow, cx: &mut Context<Self>) -> Menu {
        let list = self.list.read(cx);
        let (blocked, can_block) = (list.is_blocked(row), list.can_block(row, cx));
        let view = cx.weak_entity();
        let ask = |question| show(&view, ChatPopup::Asking(row.id.clone(), question));
        if can_block {
            menu = if blocked {
                let (unblocking, row) = (view.clone(), row.clone());
                menu.action(Icon::Block, "Unblock", move |_, cx| {
                    let _ = unblocking.update(cx, |this, cx| {
                        this.close_popup(cx);
                        this.list
                            .update(cx, |list, cx| list.set_blocked(&row, false, cx));
                    });
                })
            } else {
                menu.danger(Icon::Block, "Block", ask(Question::Block))
            };
        }
        menu = menu.action(Icon::Clear, "Clear chat", ask(Question::Clear));
        // A group's chat goes only once the user left the group.
        if !row.id.is_group() {
            menu = menu.danger(Icon::Trash, "Delete chat", ask(Question::Delete));
        }
        menu
    }
}

/// Opens `popup` in place of the menu.
fn show(
    view: &WeakEntity<ChatListView>,
    popup: ChatPopup,
) -> impl Fn(&mut Window, &mut App) + use<> {
    let view = view.clone();
    move |_, cx| {
        let _ = view.update(cx, |this, cx| {
            this.popup = Some(popup.clone());
            cx.notify();
        });
    }
}

/// The question `question` asks about `row`, with the answer that goes on.
fn ask(question: Question, row: ChatRow, cx: &mut Context<ChatListView>) -> AlertDialog {
    let view = cx.weak_entity();
    let close = {
        let view = view.clone();
        move |_: &mut Window, cx: &mut App| {
            let _ = view.update(cx, ChatListView::close_popup);
        }
    };
    let (title, description, answer) = match question {
        Question::Block => (
            format!("Block {}?", row.title),
            "Blocked contacts cannot call you or send you messages. They are not told.",
            "Block",
        ),
        Question::Clear => (
            "Clear this chat?".to_owned(),
            "Its messages are removed from this computer and your phone. Starred messages \
             stay.",
            "Clear chat",
        ),
        Question::Delete => (
            format!("Delete the chat with {}?", row.title),
            "The chat and its messages are removed from this computer and your phone.",
            "Delete chat",
        ),
    };
    AlertDialog::new(title, close.clone())
        .description(description)
        .action("Cancel", ButtonVariant::Ghost, close)
        .action(answer, ButtonVariant::Danger, move |_, cx| {
            let _ = view.update(cx, |this, cx| {
                this.close_popup(cx);
                this.answer(question, &row, cx);
            });
        })
}

impl ChatListView {
    fn answer(&mut self, question: Question, row: &ChatRow, cx: &mut Context<Self>) {
        match question {
            Question::Block => {
                self.list
                    .update(cx, |list, cx| list.set_blocked(row, true, cx));
            }
            Question::Clear | Question::Delete => {
                let deleted = question == Question::Delete;
                let action = if deleted {
                    ChatAction::Delete
                } else {
                    ChatAction::Clear
                };
                self.list.read(cx).act(row, action, cx);
                if deleted && self.selected.as_ref() == Some(&row.id) {
                    self.selected = None;
                }
                cx.emit(ChatForgotten {
                    addresses: Arc::clone(&row.addresses),
                    deleted,
                });
            }
        }
    }
}

/// Why one more chat cannot be pinned.
fn pin_limit(cx: &mut Context<ChatListView>) -> AlertDialog {
    let view = cx.weak_entity();
    let close = move |_: &mut Window, cx: &mut App| {
        let _ = view.update(cx, ChatListView::close_popup);
    };
    AlertDialog::new(
        format!("You can only pin up to {MAX_PINNED} chats"),
        close.clone(),
    )
    .description("Unpin another chat to pin this one.")
    .action("OK", ButtonVariant::Primary, close)
}
