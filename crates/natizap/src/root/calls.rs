//! Calls: the Calls place, the call history in place of the chat list, a
//! call opening its chat; and the call under way, placed from the open
//! chat, in its bar or a window of its own.

use gpui::{AppContext as _, Context, Entity, Window, px, size};
use natizap_calls::{
    CallBackRequested, CallBar, CallChatRequested, CallDesk, CallLog, CallPoppedOut, CallStage,
    CallWindow, CallsPane, RingCard, VideoPanel,
};
use natizap_chats::ChatList;
use natizap_messages::{CallRequested, ConversationView};
use natizap_profiles::ProfilePictures;

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
        CallViews {
            bar,
            card,
            panel,
            stage,
        }
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
