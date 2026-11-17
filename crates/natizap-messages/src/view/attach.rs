//! What the composer's plus button offers: files, contacts and polls.

use gpui::prelude::*;
use gpui::{Context, Window};
use natizap_protocol::Extra;
use natizap_ui::{Icon, Menu, dialog};

use super::ConversationView;
use crate::poll_composer::{PollComposer, PollDropped, PollWritten};

/// The user chose to share contacts in the open chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContactsRequested;

impl ConversationView {
    /// The plus button's menu, when open.
    pub(super) fn attach_menu(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let position = self.attach_at?;
        let view = cx.weak_entity();
        let pick = move |as_document: bool| {
            let view = view.clone();
            move |window: &mut Window, cx: &mut gpui::App| {
                let _ = view.update(cx, |this, cx| {
                    this.attach_at = None;
                    Self::attach(as_document, window, cx);
                });
            }
        };
        let view = cx.weak_entity();
        let share = move |_: &mut Window, cx: &mut gpui::App| {
            let _ = view.update(cx, |this, cx| {
                this.attach_at = None;
                cx.emit(ContactsRequested);
                cx.notify();
            });
        };
        let view = cx.weak_entity();
        let poll = move |window: &mut Window, cx: &mut gpui::App| {
            let _ = view.update(cx, |this, cx| {
                this.attach_at = None;
                this.write_poll(window, cx);
            });
        };
        let view = cx.weak_entity();
        Some(
            Menu::new()
                .action(Icon::File, "Document", pick(true))
                .action(Icon::Camera, "Photos & videos", pick(false))
                .action(Icon::Mic, "Audio", pick(false))
                .action(Icon::Contact, "Contact", share)
                .action(Icon::Poll, "Poll", poll)
                .on_dismiss(move |_, cx| {
                    let _ = view.update(cx, |this, cx| {
                        this.attach_at = None;
                        cx.notify();
                    });
                })
                .at(position),
        )
    }

    fn write_poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let composer = cx.new(|cx| PollComposer::new(window, cx));
        cx.subscribe(&composer, |this, _, PollWritten(poll), cx| {
            let poll = poll.clone();
            this.log
                .update(cx, |log, cx| log.send_extra(Extra::Poll(poll), cx));
            this.poll_draft = None;
            cx.notify();
        })
        .detach();
        cx.subscribe(&composer, |this, _, _: &PollDropped, cx| {
            this.poll_draft = None;
            cx.notify();
        })
        .detach();
        self.poll_draft = Some(composer);
        cx.notify();
    }

    /// The poll being written, over the dimmed window.
    pub(super) fn poll_dialog(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let composer = self.poll_draft.clone()?;
        let view = cx.weak_entity();
        Some(dialog(window, composer, move |_, cx| {
            let _ = view.update(cx, |this, cx| {
                this.poll_draft = None;
                cx.notify();
            });
        }))
    }
}
