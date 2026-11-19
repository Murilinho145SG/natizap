//! Entry point of the `natizap` binary: opens the main window and wires the
//! feature crates together.

// A Windows release opens without a console window behind it.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod history;
mod instance;
#[cfg(target_os = "macos")]
mod macos;
mod root;
mod shell;

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use gpui::{
    App, AppContext as _, Bounds, KeyBinding, Pixels, Size, WindowBackgroundAppearance,
    WindowBounds, WindowDecorations, WindowOptions, actions, px, size,
};
use natizap_calls::CallLog;
use natizap_chats::ChatList;
use natizap_messages::MessageLog;
use natizap_notifications::Notifier;
use natizap_profiles::ProfilePictures;
use natizap_protocol::Protocol;
use natizap_session::Session;
use natizap_status::StatusBook;
use natizap_stickers::StickerShelf;
use natizap_themes::ThemeBook;
use natizap_ui::{Assets, Gallery, bind_text_input_keys, load_fonts, titlebar_options};
use natizap_update::Updater;
use tracing_subscriber::EnvFilter;

use crate::instance::{Claim, Instance, ask_to_show};
use crate::root::Features;
use crate::shell::Shell;

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
    let app = gpui_platform::application()
        .with_assets(Assets)
        .with_restart_arguments(std::env::args_os().skip(1).collect());
    // macOS asks this of the running natizap when its Dock icon is clicked.
    let (reopen, reopened) = async_channel::unbounded();
    app.on_reopen(move |_| {
        // Heard for as long as natizap runs.
        let _ = reopen.try_send(());
    });
    app.run(move |cx: &mut App| {
        gpui_tokio::init(cx);
        // Names natizap in the system's notifications.
        cx.set_app_identity("natizap", "natizap");
        bind_text_input_keys(cx);
        cx.bind_keys([
            // `secondary` is Cmd on macOS and Ctrl elsewhere.
            KeyBinding::new("secondary-alt-g", OpenComponents, None),
            KeyBinding::new("secondary-r", Restart, None),
        ]);
        cx.set_restart_path(executable.clone());
        cx.on_action(|_: &Restart, cx| cx.restart());
        cx.on_action(|_: &OpenComponents, cx| {
            if let Err(error) = open_components_window(cx) {
                tracing::error!(?error, "could not open the components window");
            }
        });
        #[cfg(target_os = "macos")]
        macos::add_menu(cx);
        if let Err(error) = load_fonts(cx) {
            // System fonts still render every screen, just less consistently.
            tracing::error!(?error, "could not load the bundled fonts");
        }
        let features = features(&data_dir, &executable, cx);
        let shell = cx.new(|cx| Shell::new(features, cx));
        shell.update(cx, |shell, cx| {
            shell.show(None, cx);
            shell.start_tray(cx);
            #[cfg(target_os = "linux")]
            match natizap_launcher::Launcher::new(data_dir.join("icons")) {
                Ok(launcher) => shell.start_launcher(launcher, cx),
                Err(error) => tracing::warn!(%error, "the launcher keeps the package's icon"),
            }
        });
        for requests in [show_requests.clone(), reopened.clone()] {
            let shell = shell.clone();
            cx.spawn(async move |cx| {
                while requests.recv().await.is_ok() {
                    shell.update(cx, |shell, cx| shell.show(None, cx));
                }
            })
            .detach();
        }
        cx.activate(true);
    });
    Ok(())
}

/// A natizap window: drawn frame, transparent corners, centered.
fn window_options(title: &str, open: Size<Pixels>, min: Size<Pixels>, cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, open, cx))),
        titlebar: Some(titlebar_options(title.to_owned())),
        app_id: Some("natizap".into()),
        window_decorations: Some(WindowDecorations::Client),
        // See-through only where natizap draws its own rounded frame: macOS
        // rounds and shadows the window itself.
        window_background: if cfg!(target_os = "macos") {
            WindowBackgroundAppearance::Opaque
        } else {
            WindowBackgroundAppearance::Transparent
        },
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

/// The main window: it comes and goes, the features stay.
fn main_window_options(cx: &App) -> WindowOptions {
    window_options(
        "natizap",
        size(px(1100.0), px(720.0)),
        size(px(720.0), px(480.0)),
        cx,
    )
}

/// Every feature, built once for the whole run.
fn features(data_dir: &Path, executable: &Path, cx: &mut App) -> Features {
    // First, so everything draws in the chosen theme from the start.
    let themes = cx.new(|cx| ThemeBook::new(data_dir, cx));
    let protocol = cx.new(|cx| Protocol::new(data_dir.join("session.db"), cx));
    let chats = cx.new(|cx| ChatList::new(&protocol, data_dir.join("chats.db"), cx));
    let messages = cx.new(|cx| {
        MessageLog::new(
            protocol.clone(),
            data_dir.join("messages.db"),
            data_dir.join("media"),
            data_dir.join("conversation.conf"),
            cx,
        )
    });
    let pictures =
        cx.new(|cx| ProfilePictures::new(protocol.clone(), data_dir.join("avatars"), cx));
    let session = cx.new(|cx| Session::new(protocol.clone(), cx));
    let notifier = cx.new(|cx| Notifier::new(&protocol, data_dir.join("notifications.conf"), cx));
    let stickers = cx.new(|cx| {
        StickerShelf::new(
            &protocol,
            data_dir.join("stickers.db"),
            data_dir.join("media").join("stickers"),
            cx,
        )
    });
    let updater = cx.new(|cx| Updater::new(executable, data_dir.join("updates"), cx));
    let status = cx.new(|cx| {
        StatusBook::new(
            &protocol,
            data_dir.join("status.db"),
            data_dir.join("media").join("status"),
            cx,
        )
    });
    let calls = cx.new(|cx| CallLog::new(&protocol, data_dir.join("calls.db"), cx));
    Features {
        session,
        chats,
        messages,
        pictures,
        protocol,
        notifier,
        stickers,
        updater,
        status,
        calls,
        themes,
    }
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
