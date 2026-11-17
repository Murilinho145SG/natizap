//! Entry point of the `natizap` binary: opens the main window and wires the
//! feature crates together.

use std::path::PathBuf;

use anyhow::Context as _;
use gpui::{App, AppContext as _, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};
use natizap_protocol::Protocol;
use natizap_session::{PairingView, Session};
use tracing_subscriber::EnvFilter;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let store_path = store_path()?;
    gpui_platform::application().run(move |cx: &mut App| {
        gpui_tokio::init(cx);
        if let Err(error) = open_main_window(store_path, cx) {
            tracing::error!(?error, "could not open the main window");
            cx.quit();
            return;
        }
        cx.activate(true);
    });
    Ok(())
}

fn open_main_window(store_path: PathBuf, cx: &mut App) -> anyhow::Result<()> {
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
        let protocol = cx.new(|cx| Protocol::new(store_path, cx));
        let session = cx.new(|cx| Session::new(protocol, cx));
        cx.new(|cx| PairingView::new(session, cx))
    })?;
    Ok(())
}

/// Location of the session database, in the platform's per-user data
/// directory (e.g. `~/.local/share/natizap` on Linux).
fn store_path() -> anyhow::Result<PathBuf> {
    let dir = dirs::data_dir()
        .context("no per-user data directory on this platform")?
        .join("natizap");
    std::fs::create_dir_all(&dir).with_context(|| format!("could not create {}", dir.display()))?;
    Ok(dir.join("session.db"))
}
