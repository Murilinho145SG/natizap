//! natizap running, with or without its window: the features live here for
//! the whole run, so closing the window to the tray keeps the connection
//! and the notifications going, and the window opens again on the same
//! state.

use std::collections::HashSet;

use gpui::{App, AppContext as _, Context, Entity, WindowHandle, WindowId};
use natizap_messages::StoredStatus;
use natizap_notifications::NotificationOpened;
use natizap_protocol::ChatId;
use natizap_status::Post;
use natizap_themes::ThemeChosen;
use natizap_tray::{Tray, TrayAction};

use crate::main_window_options;
use crate::root::{Features, Root};

/// The running app: its features, and its main window while it is open.
pub(crate) struct Shell {
    features: Features,
    window: Option<WindowHandle<Root>>,
    /// The tray icon, once it shows.
    tray: Option<Entity<Tray>>,
}

impl Shell {
    /// Keeps `features` for the whole run, and opens the chat of a
    /// notification clicked even while the window is closed.
    pub(crate) fn new(features: Features, cx: &mut Context<Self>) -> Self {
        cx.subscribe(
            &features.notifier,
            |shell, _, NotificationOpened { chat }, cx| {
                shell.show(chat.as_ref(), cx);
            },
        )
        .detach();
        Self::hand_over_backlog(&features, cx);
        let shell = cx.weak_entity();
        cx.on_window_closed(move |cx, closed| {
            let _ = shell.update(cx, |shell, cx| shell.window_closed(closed, cx));
        })
        .detach();
        Self {
            features,
            window: None,
            tray: None,
        }
    }

    /// Gives the status and the call history what the message store kept
    /// for them from before they had stores of their own: the last day's
    /// status updates and missed calls.
    fn hand_over_backlog(features: &Features, cx: &mut Context<Self>) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| {
                i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
            });
        let messages = features.messages.read(cx);
        let statuses = messages.stored_statuses(now - DAY, cx);
        let calls = messages.stored_missed_calls(cx);
        let (status, log) = (features.status.clone(), features.calls.clone());
        cx.spawn(async move |_, cx| {
            let posts: Vec<Post> = statuses.await.into_iter().map(post).collect();
            status.update(cx, |status, _| status.import(posts));
            let calls = calls.await;
            log.update(cx, |log, _| log.import(calls));
        })
        .detach();
    }

    /// Shows the main window, opening it if it is closed, then the chat
    /// `with`, if given.
    pub(crate) fn show(&mut self, with: Option<&ChatId>, cx: &mut Context<Self>) {
        let shown = self.window.and_then(|window| {
            window
                .update(cx, |_, window, _| window.activate_window())
                .ok()
                .map(|()| window)
        });
        let window = match shown {
            Some(window) => window,
            None => match self.open(cx) {
                Ok(window) => window,
                Err(error) => {
                    tracing::error!(?error, "could not open the main window");
                    return;
                }
            },
        };
        if let Some(chat) = with {
            let _ = window.update(cx, |root, _, cx| root.open_notified(chat, cx));
        }
    }

    fn open(&mut self, cx: &mut Context<Self>) -> anyhow::Result<WindowHandle<Root>> {
        let features = self.features.clone();
        let options = main_window_options(cx);
        let window = cx.open_window(options, |window, cx| {
            cx.new(|cx| Root::new(features, window, cx))
        })?;
        self.window = Some(window);
        Ok(window)
    }

    /// Puts natizap in the tray: from then on, closing the window leaves
    /// natizap running there. Without a tray, closing still quits, so
    /// natizap never runs where nothing shows it.
    pub(crate) fn start_tray(&self, cx: &mut Context<Self>) {
        let themes = self.features.themes.clone();
        let starting = Tray::start(themes.read(cx).theme(), cx);
        cx.spawn(async move |shell, cx| {
            let tray = match starting.await {
                Ok(tray) => tray,
                Err(error) => {
                    tracing::warn!(%error, "no tray icon; closing the window quits");
                    return;
                }
            };
            let _ = shell.update(cx, |shell, cx| {
                cx.subscribe(&tray, |shell, _, action: &TrayAction, cx| match action {
                    TrayAction::Open => shell.show(None, cx),
                    TrayAction::Quit => cx.quit(),
                })
                .detach();
                let drawn = tray.clone();
                cx.subscribe(&themes, move |_, _, ThemeChosen(theme), cx| {
                    drawn.update(cx, |tray, cx| tray.set_theme(*theme, cx));
                })
                .detach();
                cx.set_quit_mode(gpui::QuitMode::Explicit);
                shell.tray = Some(tray);
            });
        })
        .detach();
    }

    /// Keeps natizap's icon in the desktop's launcher, and so in the dock,
    /// drawn in the theme in use.
    #[cfg(target_os = "linux")]
    pub(crate) fn start_launcher(
        &self,
        launcher: natizap_launcher::Launcher,
        cx: &mut Context<Self>,
    ) {
        let themes = self.features.themes.clone();
        launcher.show(themes.read(cx).theme(), cx);
        cx.subscribe(&themes, move |_, _, ThemeChosen(theme), cx| {
            launcher.show(*theme, cx);
        })
        .detach();
    }

    /// The window went: with it, whatever the user was looking at, so
    /// every message notifies again.
    fn window_closed(&mut self, closed: WindowId, cx: &mut App) {
        if self.window.map(|window| window.window_id()) != Some(closed) {
            return;
        }
        self.window = None;
        self.features.notifier.update(cx, |notifier, _| {
            notifier.set_focused(false);
            notifier.set_open(HashSet::new());
        });
    }
}

/// How long a status lasts, in seconds.
const DAY: i64 = 24 * 60 * 60;

/// A status update the message store kept, as the status shows it.
fn post(stored: StoredStatus) -> Post {
    Post {
        poster: stored.poster,
        id: stored.id,
        sent_at: stored.sent_at,
        kind: stored.kind,
        text: stored.text,
        background: None,
        author: stored.author,
        media: stored.media,
        file: None,
        seen: stored.mine,
        mine: stored.mine,
        link: None,
    }
}
