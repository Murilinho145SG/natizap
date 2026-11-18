//! Entry point of the `natizap` binary: opens the main window and wires the
//! feature crates together.

// A Windows release opens without a console window behind it.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod history;
mod instance;
mod root;

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use gpui::{
    App, AppContext as _, Bounds, KeyBinding, Pixels, Size, TitlebarOptions,
    WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowHandle, WindowOptions,
    actions, px, size,
};
use natizap_chats::ChatList;
use natizap_messages::MessageLog;
use natizap_notifications::Notifier;
use natizap_profiles::ProfilePictures;
use natizap_protocol::Protocol;
use natizap_session::Session;
use natizap_stickers::StickerShelf;
use natizap_ui::{Assets, Gallery, bind_text_input_keys, load_fonts};
use natizap_update::Updater;
use tracing_subscriber::EnvFilter;

use crate::instance::{Claim, Instance, ask_to_show};
use crate::root::{Features, Root};

actions!(
    natizap,
    [
        /// Opens the window showing every component in every state.
        OpenComponents,
        /// Closes natizap and opens it again.
        Restart
    ]
);

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let data_dir = data_dir()?;
    let instance = match Instance::claim(&data_dir).context("could not claim the data folder")? {
        Claim::First(instance) => instance,
        Claim::Running => {
            if let Err(error) = ask_to_show(&data_dir) {
                tracing::warn!(%error, "natizap runs already but did not answer");
            }
            return Ok(());
        }
    };
    let show_requests = instance.listen();
    // Read now: a rebuild replaces the file, and the path the running
    // process reports for it afterwards no longer opens.
    let executable = std::env::current_exe().context("could not find natizap's executable")?;
    gpui_platform::application()
        .with_assets(Assets)
        .with_restart_arguments(std::env::args_os().skip(1).collect())
        .run(move |cx: &mut App| {
            gpui_tokio::init(cx);
            // Names natizap in the system's notifications.
            cx.set_app_identity("natizap", "natizap");
            bind_text_input_keys(cx);
            cx.bind_keys([
                KeyBinding::new("ctrl-alt-g", OpenComponents, None),
                KeyBinding::new("ctrl-r", Restart, None),
            ]);
            cx.set_restart_path(executable.clone());
            cx.on_action(|_: &Restart, cx| cx.restart());
            cx.on_action(|_: &OpenComponents, cx| {
                if let Err(error) = open_components_window(cx) {
                    tracing::error!(?error, "could not open the components window");
                }
            });
            if let Err(error) = load_fonts(cx) {
                // System fonts still render every screen, just less consistently.
                tracing::error!(?error, "could not load the bundled fonts");
            }
            let main_window = match open_main_window(&data_dir, &executable, cx) {
                Ok(window) => window,
                Err(error) => {
                    tracing::error!(?error, "could not open the main window");
                    cx.quit();
                    return;
                }
            };
            let show_requests = show_requests.clone();
            cx.spawn(async move |cx| {
                while show_requests.recv().await.is_ok() {
                    let _ = main_window.update(cx, |_, window, _| window.activate_window());
                }
            })
            .detach();
            cx.activate(true);
        });
    Ok(())
}

/// A natizap window: drawn frame, transparent corners, centered.
fn window_options(title: &str, open: Size<Pixels>, min: Size<Pixels>, cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, open, cx))),
        titlebar: Some(TitlebarOptions {
            title: Some(title.to_owned().into()),
            ..Default::default()
        }),
        app_id: Some("natizap".into()),
        window_decorations: Some(WindowDecorations::Client),
        window_background: WindowBackgroundAppearance::Transparent,
        window_min_size: Some(min),
        ..Default::default()
    }
}

fn open_components_window(cx: &mut App) -> anyhow::Result<()> {
    let options = window_options(
        "natizap components",
        size(px(760.0), px(820.0)),
        size(px(480.0), px(360.0)),
        cx,
    );
    cx.open_window(options, |_, cx| cx.new(|_| Gallery::default()))?;
    Ok(())
}

fn open_main_window(
    data_dir: &Path,
    executable: &Path,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<Root>> {
    let options = window_options(
        "natizap",
        size(px(1100.0), px(720.0)),
        size(px(720.0), px(480.0)),
        cx,
    );
    cx.open_window(options, |window, cx| {
        let protocol = cx.new(|cx| Protocol::new(data_dir.join("session.db"), cx));
        let chats = cx.new(|cx| ChatList::new(&protocol, data_dir.join("chats.db"), cx));
        let messages = cx.new(|cx| {
            MessageLog::new(
                protocol.clone(),
                data_dir.join("messages.db"),
                data_dir.join("media"),
                cx,
            )
        });
        let pictures =
            cx.new(|cx| ProfilePictures::new(protocol.clone(), data_dir.join("avatars"), cx));
        let session = cx.new(|cx| Session::new(protocol.clone(), cx));
        let notifier =
            cx.new(|cx| Notifier::new(&protocol, data_dir.join("notifications.conf"), cx));
        let stickers = cx.new(|cx| {
            StickerShelf::new(
                &protocol,
                data_dir.join("stickers.db"),
                data_dir.join("media").join("stickers"),
                cx,
            )
        });
        let updater = cx.new(|cx| Updater::new(executable, data_dir.join("updates"), cx));
        let features = Features {
            session,
            chats,
            messages,
            pictures,
            protocol,
            notifier,
            stickers,
            updater,
        };
        cx.new(|cx| Root::new(features, window, cx))
    })
}

/// Where natizap keeps its local data: the platform's per-user data
/// directory (e.g. `~/.local/share/natizap` on Linux).
fn data_dir() -> anyhow::Result<PathBuf> {
    let dir = dirs::data_dir()
        .context("no per-user data directory on this platform")?
        .join("natizap");
    std::fs::create_dir_all(&dir).with_context(|| format!("could not create {}", dir.display()))?;
    Ok(dir)
}
