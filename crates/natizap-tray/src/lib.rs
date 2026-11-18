//! natizap's icon in the system tray: it keeps natizap reachable while its
//! window is closed, and offers to open it again or to quit.
//!
//! GPUI has no tray of its own. On Linux the icon is a `StatusNotifierItem`
//! over D-Bus (GNOME shows it with the `AppIndicator` extension, which
//! Ubuntu ships on); on Windows it is a notification-area icon.

use gpui::{App, AppContext as _, Context, Entity, EventEmitter, Task};

#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod platform;
#[cfg(windows)]
#[path = "windows.rs"]
mod platform;

/// The app icon, drawn into the tray at this side.
const SIDE: u32 = 64;

/// What the user asked of the tray icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    /// Show natizap's window, opening it if it was closed.
    Open,
    /// End natizap.
    Quit,
}

/// Why the tray icon could not show.
#[derive(Debug, thiserror::Error)]
pub enum TrayError {
    /// The desktop has no tray, or refused the icon.
    #[error("the system tray is unavailable: {0}")]
    Unavailable(String),
}

/// The tray icon. Emits [`TrayAction`]s; the icon goes when it is dropped.
pub struct Tray {
    _icon: platform::Icon,
    _hearing: Task<()>,
}

impl std::fmt::Debug for Tray {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tray").finish_non_exhaustive()
    }
}

impl EventEmitter<TrayAction> for Tray {}

impl Tray {
    /// Puts natizap's icon in the tray.
    ///
    /// # Errors
    ///
    /// The task fails when the desktop has no tray to show it in.
    pub fn start(cx: &mut App) -> Task<Result<Entity<Self>, TrayError>> {
        let (actions, heard) = async_channel::unbounded();
        let showing = platform::show(icon_rgba(), actions, cx);
        cx.spawn(async move |cx| {
            let icon = showing.await?;
            Ok(cx.new(|cx: &mut Context<Self>| Self {
                _icon: icon,
                _hearing: cx.spawn(async move |this, cx| {
                    while let Ok(action) = heard.recv().await {
                        if this.update(cx, |_, cx| cx.emit(action)).is_err() {
                            break;
                        }
                    }
                }),
            }))
        })
    }
}

/// The app icon as `SIDE`×`SIDE` RGBA pixels.
fn icon_rgba() -> Vec<u8> {
    match image::load_from_memory_with_format(
        include_bytes!("../../../brand/natizap-icon.png"),
        image::ImageFormat::Png,
    ) {
        Ok(icon) => icon
            .resize_exact(SIDE, SIDE, image::imageops::FilterType::Lanczos3)
            .into_rgba8()
            .into_raw(),
        Err(error) => {
            // Bundled at build time, so this cannot happen with a sound
            // build; a blank icon still offers the menu.
            tracing::error!(%error, "could not read the bundled app icon");
            vec![0; (SIDE * SIDE * 4) as usize]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SIDE, icon_rgba};

    #[test]
    fn draws_the_app_icon_at_tray_size() {
        let pixels = icon_rgba();
        assert_eq!(pixels.len(), (SIDE * SIDE * 4) as usize);
        assert!(pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[3] > 0));
    }
}
