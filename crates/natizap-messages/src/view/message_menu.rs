//! What a right click on a message offers, and the question deleting asks.

use gpui::prelude::*;
use gpui::{ClipboardItem, Context, Pixels, Point, SharedString, WeakEntity, div, px, rgb, svg};
use natizap_protocol::{ContentKind, Forward, PinFor};
use natizap_ui::theme::ActiveTheme as _;
use natizap_ui::{AlertDialog, ButtonVariant, Icon, Menu};

use super::ConversationView;
use crate::items::{Body, Bubble, Item};
use crate::log::{MessageLog, Pinned};

/// The message a menu or the delete question is about.
#[derive(Debug, Clone)]
pub(super) enum MessagePopup {
    /// The right-click menu, where the click happened.
    Menu(SharedString, Point<Pixels>),
    /// Asking how to delete.
    Deleting(SharedString),
    /// Asking how long to pin for.
    Pinning(SharedString),
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

    pub(super) fn bubble(&self, id: &SharedString, cx: &Context<Self>) -> Option<Bubble> {
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
                let pinned = self
                    .log
                    .read(cx)
                    .pinned()
                    .is_some_and(|pinned| pinned.id == bubble.id);
                // A sticker's own action comes first.
                let start = self.with_favorite_sticker(Menu::new(), &bubble, cx);
                Some(
                    Self::menu(&bubble, pinned, start, cx)
                        .at(*position)
                        .into_any_element(),
                )
            }
            MessagePopup::Deleting(id) => {
                let bubble = self.bubble(id, cx)?;
                Some(Self::delete_dialog(&bubble, cx).into_any_element())
            }
            MessagePopup::Pinning(id) => {
                let bubble = self.bubble(id, cx)?;
                Some(Self::pin_dialog(&bubble, cx).into_any_element())
            }
        }
    }

    /// Reply, copy, react and delete, as on the phone, after what `start`
    /// already offers. Copy only shows for messages with text, and only
    /// delete for deleted ones.
    fn menu(bubble: &Bubble, pinned: bool, start: Menu, cx: &mut Context<Self>) -> Menu {
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
        let mut menu = start.on_dismiss(close);
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
            menu = with_saving(menu, bubble, &view);
            menu = with_forward(menu, bubble, &view);
            menu = with_pin(menu, bubble, pinned, &view);
            menu = with_star(menu, bubble, &view);
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
            bubble.participant.clone(),
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

    /// How long to pin for: 24 hours, 7 days or 30 days, as on the phone.
    fn pin_dialog(bubble: &Bubble, cx: &mut Context<Self>) -> AlertDialog {
        let view = cx.weak_entity();
        let close = {
            let view = view.clone();
            move |_: &mut gpui::Window, cx: &mut gpui::App| {
                let _ = view.update(cx, ConversationView::close_message_popup);
            }
        };
        let mut dialog = AlertDialog::new("Choose how long your pin lasts", close.clone())
            .description("You can unpin at any time.")
            .action("Cancel", ButtonVariant::Ghost, close);
        for pin in PinFor::ALL {
            let view = view.clone();
            let (id, from_me, sender) = (
                bubble.id.to_string(),
                bubble.from_me,
                bubble.participant.clone(),
            );
            dialog = dialog.action(pin.label(), ButtonVariant::Secondary, move |_, cx| {
                let _ = view.update(cx, |view, cx| {
                    view.close_message_popup(cx);
                    view.log.update(cx, |log, cx| {
                        log.set_pinned(id.clone(), from_me, sender.as_ref(), Some(pin), cx);
                    });
                });
            });
        }
        dialog
    }

    /// The pinned message under the header; clicking it scrolls to the
    /// message when it is among those loaded.
    pub(super) fn pinned_bar(&self, pinned: Pinned, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let index = self
            .log
            .read(cx)
            .items()
            .iter()
            .position(|item| matches!(item, Item::Bubble(bubble) if bubble.id == pinned.id));
        div()
            .id("pinned")
            .h(px(40.0))
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .bg(rgb(theme.panel))
            .border_b_1()
            .border_color(rgb(theme.line))
            .text_size(px(13.5))
            .when(index.is_some(), |bar| {
                bar.cursor_pointer().hover(|bar| bar.bg(rgb(theme.raised)))
            })
            .on_click(cx.listener(move |this, _, _, _| {
                if let Some(index) = index {
                    this.messages.scroll_to_reveal_item(index);
                }
            }))
            .child(
                svg()
                    .path(Icon::Pin.path())
                    .size(px(15.0))
                    .flex_none()
                    .text_color(rgb(theme.muted)),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(rgb(theme.text))
                    .child(pinned.text),
            )
    }

    pub(super) fn close_message_popup(&mut self, cx: &mut Context<Self>) {
        if self.message_popup.take().is_some() {
            cx.notify();
        }
    }
}

/// Forward: asks the app which chats to send the message to.
/// Copy image for photos, and Save as for any attachment.
fn with_saving(mut menu: Menu, bubble: &Bubble, view: &WeakEntity<ConversationView>) -> Menu {
    let Some(media) = bubble.media.clone() else {
        return menu;
    };
    if media.fetch.kind == ContentKind::Photo {
        let (view, media) = (view.clone(), media.clone());
        menu = menu.action(Icon::Copy, "Copy image", move |_, cx| {
            let _ = view.update(cx, |view, cx| {
                view.close_message_popup(cx);
                view.copy_media_image(&media, cx);
            });
        });
    }
    let view = view.clone();
    menu.action(Icon::Download, "Save as…", move |_, cx| {
        let _ = view.update(cx, |view, cx| {
            view.close_message_popup(cx);
            ConversationView::save_media_as(&media, cx);
        });
    })
}

fn with_forward(menu: Menu, bubble: &Bubble, view: &WeakEntity<ConversationView>) -> Menu {
    let view = view.clone();
    let forward = Forward {
        kind: bubble
            .media
            .as_ref()
            .map_or(ContentKind::Text, |media| media.fetch.kind),
        text: bubble.source_text.clone(),
        media: bubble.media.as_ref().map(|media| media.fetch.media.clone()),
    };
    let file = bubble.media.as_ref().and_then(|media| media.file.clone());
    menu.action(Icon::Forward, "Forward", move |_, cx| {
        let _ = view.update(cx, |view, cx| {
            view.close_message_popup(cx);
            MessageLog::request_forward(&view.log, forward.clone(), file.clone(), cx);
        });
    })
}

/// Pin asks for how long; unpin is immediate.
fn with_pin(
    menu: Menu,
    bubble: &Bubble,
    pinned: bool,
    view: &WeakEntity<ConversationView>,
) -> Menu {
    let view = view.clone();
    let (id, from_me, sender) = (
        bubble.id.clone(),
        bubble.from_me,
        bubble.participant.clone(),
    );
    menu.action(
        Icon::Pin,
        if pinned { "Unpin" } else { "Pin" },
        move |_, cx| {
            let _ = view.update(cx, |view, cx| {
                if pinned {
                    view.close_message_popup(cx);
                    view.log.update(cx, |log, cx| {
                        log.set_pinned(id.to_string(), from_me, sender.as_ref(), None, cx);
                    });
                } else {
                    view.message_popup = Some(MessagePopup::Pinning(id.clone()));
                    cx.notify();
                }
            });
        },
    )
}

fn with_star(menu: Menu, bubble: &Bubble, view: &WeakEntity<ConversationView>) -> Menu {
    let view = view.clone();
    let (id, from_me, sender, starred) = (
        bubble.id.to_string(),
        bubble.from_me,
        bubble.participant.clone(),
        bubble.starred,
    );
    menu.action(
        Icon::Star,
        if starred { "Unstar" } else { "Star" },
        move |_, cx| {
            let _ = view.update(cx, |view, cx| {
                view.close_message_popup(cx);
                view.log.update(cx, |log, cx| {
                    log.set_starred(id.clone(), from_me, sender.as_ref(), !starred, cx);
                });
            });
        },
    )
}
