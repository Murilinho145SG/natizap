//! Entry point of the `natizap` binary: opens the main window and wires the
//! feature crates together.

mod root;

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use gpui::{App, AppContext as _, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};
use natizap_chats::ChatList;
use natizap_protocol::Protocol;
use natizap_session::Session;
use tracing_subscriber::EnvFilter;

use crate::root::Root;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let data_dir = data_dir()?;
    gpui_platform::application().run(move |cx: &mut App| {
        gpui_tokio::init(cx);
        if let Err(error) = open_main_window(&data_dir, cx) {
            tracing::error!(?error, "could not open the main window");
            cx.quit();
            return;
        }
        cx.activate(true);
    });
    Ok(())
}

fn open_main_window(data_dir: &Path, cx: &mut App) -> anyhow::Result<()> {
    let bounds = Bounds::centered(None, size(px(1100.0), px(720.0)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("natizap".into()),
            ..Default::default()
        }),
        app_id: Some("natizap".into()),
        ..Default::default()
    };
    cx.open_window(options, |_, cx| {
        let protocol = cx.new(|cx| Protocol::new(data_dir.join("session.db"), cx));
        let chats = cx.new(|cx| ChatList::new(&protocol, data_dir.join("chats.db"), cx));
        let session = cx.new(|cx| Session::new(protocol, cx));
        cx.new(|cx| Root::new(session, chats, cx))
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
