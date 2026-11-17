//! Entry point of the `natizap` binary: opens the main window and wires the
//! feature crates together.

mod history;
mod root;

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use gpui::{
    App, AppContext as _, Bounds, KeyBinding, Pixels, Size, TitlebarOptions,
    WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowOptions, actions, px, size,
};
use natizap_chats::ChatList;
use natizap_messages::MessageLog;
use natizap_profiles::ProfilePictures;
use natizap_protocol::Protocol;
use natizap_session::Session;
use natizap_ui::{Assets, Gallery, bind_text_input_keys, load_fonts};
use tracing_subscriber::EnvFilter;

use crate::root::Root;

actions!(
    natizap,
    [
        /// Opens the window showing every component in every state.
        OpenComponents
    ]
);

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let data_dir = data_dir()?;
    gpui_platform::application()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            gpui_tokio::init(cx);
            bind_text_input_keys(cx);
            cx.bind_keys([KeyBinding::new("ctrl-alt-g", OpenComponents, None)]);
            cx.on_action(|_: &OpenComponents, cx| {
                if let Err(error) = open_components_window(cx) {
                    tracing::error!(?error, "could not open the components window");
                }
            });
            if let Err(error) = load_fonts(cx) {
                // System fonts still render every screen, just less consistently.
                tracing::error!(?error, "could not load the bundled fonts");
            }
            if let Err(error) = open_main_window(&data_dir, cx) {
                tracing::error!(?error, "could not open the main window");
                cx.quit();
                return;
            }
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

fn open_main_window(data_dir: &Path, cx: &mut App) -> anyhow::Result<()> {
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
        let session = cx.new(|cx| Session::new(protocol, cx));
        cx.new(|cx| Root::new(session, chats, &messages, &pictures, window, cx))
    })?;
    Ok(())
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
