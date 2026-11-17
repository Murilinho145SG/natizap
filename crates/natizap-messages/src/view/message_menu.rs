//! What a right click on a message offers, and the question deleting asks.

use gpui::prelude::*;
use gpui::{ClipboardItem, Context, Pixels, Point, SharedString};
use natizap_ui::{AlertDialog, ButtonVariant, Icon, Menu};

use super::ConversationView;
use crate::items::{Body, Bubble, Item};

/// The message a menu or the delete question is about.
#[derive(Debug, Clone)]
pub(super) enum MessagePopup {
    /// The right-click menu, where the click happened.
    Menu(SharedString, Point<Pixels>),
    /// Asking how to delete.
    Deleting(SharedString),
}

impl ConversationView {
    /// Opens the actions of message `id` at `position`.
    pub(crate) fn open_message_menu(
        &mut self,
        id: SharedString,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.close_popups(cx);
        self.message_popup = Some(MessagePopup::Menu(id, position));
        cx.notify();
    }

    fn bubble(&self, id: &SharedString, cx: &Context<Self>) -> Option<Bubble> {
        self.log
            .read(cx)
            .items()
            .iter()
            .find_map(|item| match item {
                Item::Bubble(bubble) if bubble.id == *id => Some(bubble.as_ref().clone()),
                _ => None,
            })
    }

    /// The open menu or question, if any.
    pub(super) fn message_popup(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        match self.message_popup.as_ref()? {
            MessagePopup::Menu(id, position) => {
                let bubble = self.bubble(id, cx)?;
                Some(Self::menu(&bubble, cx).at(*position).into_any_element())
            }
            MessagePopup::Deleting(id) => {
                let bubble = self.bubble(id, cx)?;
                Some(Self::delete_dialog(&bubble, cx).into_any_element())
            }
        }
    }

    /// Reply, copy, react and delete, as on the phone. Copy only shows for
    /// messages with text, and only delete for deleted ones.
    fn menu(bubble: &Bubble, cx: &mut Context<Self>) -> Menu {
        let view = cx.weak_entity();
        let close = {
            let view = view.clone();
            move |_: &mut gpui::Window, cx: &mut gpui::App| {
                let _ = view.update(cx, ConversationView::close_message_popup);
            }
        };
        let deleted = matches!(bubble.body, Body::Deleted);
        let text = match &bubble.body {
            Body::Content {
                text: Some(text), ..
            } => Some(text.text.to_string()),
            _ => None,
        };
        let mut menu = Menu::new().on_dismiss(close);
        if !deleted {
            let (replying, replied) = (view.clone(), bubble.clone());
            menu = menu.action(Icon::Reply, "Reply", move |_, cx| {
                let _ = replying.update(cx, |view, cx| {
                    view.close_message_popup(cx);
                    view.reply_to(&replied, cx);
                });
            });
            if let Some(text) = text {
                let copying = view.clone();
                menu = menu.action(Icon::Copy, "Copy", move |_, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                    let _ = copying.update(cx, ConversationView::close_message_popup);
                });
            }
            let (reacting, id) = (view.clone(), bubble.id.clone());
            menu = menu.action(Icon::Emoji, "React", move |_, cx| {
                let _ = reacting.update(cx, |view, cx| {
                    view.close_message_popup(cx);
                    view.toggle_reactions(&id, cx);
                });
            });
            menu = menu.separator();
        }
        let id = bubble.id.clone();
        menu.danger(Icon::Trash, "Delete", move |_, cx| {
            let _ = view.update(cx, |view, cx| {
                view.message_popup = Some(MessagePopup::Deleting(id.clone()));
                cx.notify();
            });
        })
    }

    /// Delete for everyone, offered for this account's own messages only,
    /// or for this account's devices.
    fn delete_dialog(bubble: &Bubble, cx: &mut Context<Self>) -> AlertDialog {
        let view = cx.weak_entity();
        let close = {
            let view = view.clone();
            move |_: &mut gpui::Window, cx: &mut gpui::App| {
                let _ = view.update(cx, ConversationView::close_message_popup);
            }
        };
        let mine = bubble.from_me && !matches!(bubble.body, Body::Deleted);
        let mut dialog = AlertDialog::new("Delete message?", close.clone()).description(if mine {
            "Delete it for everyone in this chat, or only from your devices."
        } else {
            "It is deleted from your devices; others still see it."
        });
        dialog = dialog.action("Cancel", ButtonVariant::Ghost, close);
        let (id, from_me, sender) = (
            bubble.id.to_string(),
            bubble.from_me,
            bubble.reply.sender.clone(),
        );
        {
            let (view, id) = (view.clone(), id.clone());
            dialog = dialog.action("Delete for me", ButtonVariant::Secondary, move |_, cx| {
                let _ = view.update(cx, |view, cx| {
                    view.close_message_popup(cx);
                    view.log.update(cx, |log, cx| {
                        log.delete_for_me(id.clone(), from_me, sender.as_ref(), cx);
                    });
                });
            });
        }
        if mine {
            dialog = dialog.action(
                "Delete for everyone",
                ButtonVariant::Danger,
                move |_, cx| {
                    let _ = view.update(cx, |view, cx| {
                        view.close_message_popup(cx);
                        view.log
                            .update(cx, |log, cx| log.delete_for_everyone(id.clone(), cx));
                    });
                },
            );
        }
        dialog
    }

    pub(super) fn close_message_popup(&mut self, cx: &mut Context<Self>) {
        if self.message_popup.take().is_some() {
            cx.notify();
        }
    }
}
