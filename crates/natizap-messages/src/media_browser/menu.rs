//! What a right click offers on something in the window: opening it, going
//! to the message it came in, and copying a link.

use gpui::prelude::*;
use gpui::{
    App, ClipboardItem, Context, MouseButton, MouseDownEvent, Pixels, Point, SharedString,
    WeakEntity, Window, anchored, deferred, px,
};
use natizap_ui::{Icon, Menu};

use super::{MediaBrowser, MediaChosen, MessageWanted, Pick};

impl MediaBrowser {
    /// The menu a right click opened, when one is open. It floats over the
    /// window, which itself floats over the app.
    pub(super) fn menu(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let (position, pick) = self.menu?;
        let browser = cx.weak_entity();
        let act = move |action: Action| {
            let browser = browser.clone();
            move |_: &mut Window, cx: &mut App| {
                let _ = browser.update(cx, |this, cx| this.act(pick, action, cx));
            }
        };
        let dismiss = cx.weak_entity();
        let mut menu = Menu::new()
            .action(Icon::Maximize, open_label(pick), act(Action::Open))
            .action(Icon::Chats, "Go to message", act(Action::GoTo));
        if matches!(pick, Pick::Link(_)) {
            menu = menu.action(Icon::Copy, "Copy link", act(Action::Copy));
        }
        let menu = menu.on_dismiss(move |_, cx| {
            let _ = dismiss.update(cx, |this, cx| {
                this.menu = None;
                cx.notify();
            });
        });
        Some(
            deferred(
                anchored()
                    .position(position)
                    .snap_to_window_with_margin(px(8.0))
                    .child(menu),
            )
            // Above the window, which is a dialog.
            .with_priority(3),
        )
    }

    fn act(&mut self, pick: Pick, action: Action, cx: &mut Context<Self>) {
        self.menu = None;
        cx.notify();
        let Some(library) = self.library.clone() else {
            return;
        };
        let (media, link) = match pick {
            Pick::Media(index) => (library.media.get(index), None),
            Pick::Doc(index) => (library.docs.get(index), None),
            Pick::Link(index) => (None, library.links.get(index)),
        };
        match action {
            Action::Open => {
                if let Some(item) = media {
                    cx.emit(MediaChosen(item.media.clone()));
                } else if let Some(link) = link {
                    cx.open_url(&link.target);
                }
            }
            Action::GoTo => {
                let wanted = media
                    .map(|item| {
                        (
                            SharedString::from(item.media.fetch.message_id.clone()),
                            item.sent_at,
                        )
                    })
                    .or_else(|| link.map(|link| (link.message_id.clone(), link.sent_at)));
                if let Some((id, sent_at)) = wanted {
                    cx.emit(MessageWanted {
                        id,
                        sent_at,
                        chats: self.chats.clone(),
                    });
                }
            }
            Action::Copy => {
                if let Some(link) = link {
                    cx.write_to_clipboard(ClipboardItem::new_string(link.target.to_string()));
                }
            }
        }
    }
}

/// What the menu can do.
#[derive(Debug, Clone, Copy)]
enum Action {
    Open,
    GoTo,
    Copy,
}

fn open_label(pick: Pick) -> &'static str {
    match pick {
        Pick::Media(_) | Pick::Doc(_) => "Open",
        Pick::Link(_) => "Open link",
    }
}

/// Opens the menu for `pick` where the right click landed.
pub(super) fn on_right_click(
    browser: &WeakEntity<MediaBrowser>,
    pick: Pick,
) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static {
    let browser = browser.clone();
    move |event, _, cx| {
        cx.stop_propagation();
        let at: Point<Pixels> = event.position;
        let _ = browser.update(cx, |this, cx| {
            this.menu = Some((at, pick));
            cx.notify();
        });
    }
}

/// The button a right click is.
pub(super) const RIGHT: MouseButton = MouseButton::Right;
