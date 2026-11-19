//! Calls: the Calls place, the call history in place of the chat list, a
//! call opening its chat; and the call under way, placed from the open
//! chat, in its bar or a window of its own.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, Window, div, px, rgb, size};
use natizap_calls::{
    CallBackRequested, CallBar, CallChatRequested, CallDesk, CallLog, CallPoppedOut, CallStage,
    CallWindow, CallsPane, RingCard, SharePicker, VideoPanel,
};
use natizap_chats::ChatList;
use natizap_messages::{CallRequested, ConversationView};
use natizap_profiles::ProfilePictures;
use natizap_ui::theme::{ActiveTheme as _, base_font};

use super::{Place, Root};

/// The call under way and ringing, as the main window shows them.
pub(super) struct CallViews {
    /// The call across the top of the window.
    pub(super) bar: Entity<CallBar>,
    /// A call ringing, over the window's corner.
    pub(super) card: Entity<RingCard>,
    /// The call floating as a small picture, while arranged so.
    pub(super) panel: Entity<VideoPanel>,
    /// The call shown large over the chat, while arranged so.
    pub(super) stage: Entity<CallStage>,
    /// What to share in the call, while natizap asks.
    pub(super) picker: Entity<SharePicker>,
}

impl Root {
    /// The call's bar, ringing card, floating picture and stage over `desk`. Calls the open chat
    /// when its header asks, opens the chat of the call in the bar when it
    /// is clicked, and moves the call to its own window when the bar asks.
    pub(super) fn follow_desk(
        desk: &Entity<CallDesk>,
        pictures: &Entity<ProfilePictures>,
        conversation: &Entity<ConversationView>,
        cx: &mut Context<Self>,
    ) -> CallViews {
        let bar = cx.new(|cx| CallBar::new(desk.clone(), pictures.clone(), cx));
        let card = cx.new(|cx| RingCard::new(desk.clone(), pictures.clone(), cx));
        let panel = cx.new(|cx| VideoPanel::new(desk.clone(), pictures.clone(), cx));
        let stage = cx.new(|cx| CallStage::new(desk.clone(), pictures.clone(), cx));
        let picker = cx.new(|cx| SharePicker::new(desk.clone(), cx));
        cx.subscribe(&stage, |root, _, _: &CallPoppedOut, cx| {
            root.pop_out_call(cx);
        })
        .detach();
        cx.subscribe(
            conversation,
            |root, _, CallRequested { peer, video }, cx| {
                root.desk
                    .update(cx, |desk, cx| desk.place(peer.clone(), *video, cx));
            },
        )
        .detach();
        cx.subscribe(&panel, |root, _, _: &CallPoppedOut, cx| {
            root.pop_out_call(cx);
        })
        .detach();
        cx.subscribe(&bar, |root, _, CallChatRequested(chat), cx| {
            let title = root.calls.read(cx).title(chat);
            root.place = Place::Chats;
            root.chat_with(chat, title, cx);
        })
        .detach();
        cx.subscribe(&bar, |root, _, _: &CallPoppedOut, cx| {
            root.pop_out_call(cx);
        })
        .detach();
        // Escape leaves the full screen, as everywhere else.
        cx.observe_keystrokes(|root, event, _, cx| {
            if event.keystroke.key == "escape" && root.desk.read(cx).full_screen() {
                root.desk
                    .update(cx, |desk, cx| desk.set_full_screen(false, cx));
            }
        })
        .detach();
        CallViews {
            bar,
            card,
            panel,
            stage,
            picker,
        }
    }

    /// The call alone, without the window's frame, while it fills the
    /// screen; the window is made to fill it, or to give it back, as the
    /// desk asks.
    pub(super) fn full_screen_call(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let full = self.desk.read(cx).full_screen();
        if full != self.full_screen {
            self.full_screen = full;
            if window.is_fullscreen() != full {
                window.toggle_fullscreen();
            }
        }
        full.then(|| {
            div()
                .size_full()
                .font(base_font())
                .text_color(rgb(cx.theme().text))
                .child(self.call_views.stage.clone())
                .child(self.call_views.card.clone())
                .child(self.call_views.picker.clone())
                .into_any_element()
        })
    }

    /// Shows the call in its own window, opening it if it is not open.
    fn pop_out_call(&mut self, cx: &mut Context<Self>) {
        let shown = self.call_window.is_some_and(|window| {
            window
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        });
        if shown {
            return;
        }
        let options = crate::window_options(
            "natizap call",
            size(px(360.0), px(520.0)),
            size(px(280.0), px(380.0)),
            cx,
        );
        let (desk, pictures) = (self.desk.clone(), self.pictures.clone());
        match cx.open_window(options, |window, cx| {
            cx.new(|cx| CallWindow::new(desk, pictures, window, cx))
        }) {
            Ok(window) => self.call_window = Some(window),
            Err(error) => tracing::error!(?error, "could not open the call's window"),
        }
    }

    /// Keeps the call history told what chats are called.
    pub(super) fn follow_calls(
        calls: &Entity<CallLog>,
        chats: &Entity<ChatList>,
        cx: &mut Context<Self>,
    ) {
        let log = calls.clone();
        cx.observe(chats, move |_, chats, cx| {
            let titles = chats.read(cx).titles();
            log.update(cx, |log, cx| log.set_titles(titles, cx));
        })
        .detach();
    }

    /// Shows the Calls place in place of the chat list.
    pub(super) fn show_calls_place(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.calls_pane.is_none() {
            let (log, pictures) = (self.calls.clone(), self.pictures.clone());
            let pane = cx.new(|cx| CallsPane::new(log, pictures, cx));
            cx.subscribe_in(&pane, window, |root, _, CallChatRequested(chat), _, cx| {
                let title = root.calls.read(cx).title(chat);
                root.place = Place::Chats;
                root.chat_with(chat, title, cx);
            })
            .detach();
            cx.subscribe(&pane, |root, _, CallBackRequested { peer, video }, cx| {
                root.desk
                    .update(cx, |desk, cx| desk.place(peer.clone(), *video, cx));
            })
            .detach();
            self.calls_pane = Some(pane);
        }
        self.own_shown = false;
        self.place = Place::Calls;
        cx.notify();
    }
}
