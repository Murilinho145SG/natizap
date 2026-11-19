//! natizap's entry in the Linux desktop's launcher, which gives the dock,
//! the app grid and the window switcher their icon: it draws the icon in
//! the theme in use.
//!
//! Desktops take a running app's icon from the desktop entry named after
//! its app id, not from the window, and GNOME offers no way for a window to
//! set its own. So natizap writes its entry into the user's applications
//! folder, which shadows the one its package installs, pointing at an
//! icon drawn in the theme's colours. Each theme's icon has its own file:
//! desktops cache icons by path, and a new path is what makes them draw
//! the new one at once.

use std::path::{Path, PathBuf};

use gpui::{App, AppContext as _};
use natizap_ui::app_icon;
use natizap_ui::theme::Theme;

/// The side of the icon drawn for the launcher, in pixels.
const SIDE: u32 = 256;

/// The desktop entry natizap's package installs; the user's copy changes
/// only the command and the icon.
const ENTRY: &str = include_str!("../../../packaging/linux/natizap.desktop");

/// Why the launcher entry could not be written.
#[derive(Debug, thiserror::Error)]
pub enum LauncherError {
    /// The platform has no per-user data folder to keep applications in.
    #[error("no per-user data directory to keep the desktop entry in")]
    NoDataDir,
    /// Writing the icon or the entry failed.
    #[error("could not write {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    /// Drawing the icon into a PNG failed.
    #[error("could not encode the icon: {0}")]
    Encode(#[from] image::ImageError),
}

/// natizap's launcher entry, written in the user's applications folder.
#[derive(Debug, Clone)]
pub struct Launcher {
    /// Where the icons drawn for the entry are kept.
    icons: PathBuf,
    /// The user's applications folder.
    applications: PathBuf,
    /// The AppImage natizap runs from, if it does: what starts it then,
    /// in place of the `natizap` a package puts on the path.
    image: Option<PathBuf>,
}

impl Launcher {
    /// The entry, with its icons kept in `icons`.
    ///
    /// # Errors
    ///
    /// Fails when the platform has no per-user data folder.
    pub fn new(icons: PathBuf) -> Result<Self, LauncherError> {
        let applications = dirs::data_dir()
            .ok_or(LauncherError::NoDataDir)?
            .join("applications");
        // An AppImage's runtime names the image here; the executable itself
        // lives in a mount that goes when natizap ends.
        let image = std::env::var_os("APPIMAGE").map(PathBuf::from);
        Ok(Self {
            icons,
            applications,
            image,
        })
    }

    /// Shows natizap's icon drawn in `theme` in the launcher, off the UI
    /// thread.
    pub fn show(&self, theme: Theme, cx: &App) {
        let launcher = self.clone();
        cx.background_spawn(async move {
            if let Err(error) = launcher.write(theme) {
                tracing::warn!(%error, "could not draw the launcher icon in the theme");
            }
        })
        .detach();
    }

    fn write(&self, theme: Theme) -> Result<(), LauncherError> {
        let icon = self.icons.join(format!(
            "natizap-{:06x}-{:06x}.png",
            theme.deep, theme.accent
        ));
        if !icon.exists() {
            create_dir(&self.icons)?;
            let staged = icon.with_extension("png.part");
            image::save_buffer_with_format(
                &staged,
                &app_icon(theme, SIDE),
                SIDE,
                SIDE,
                image::ExtendedColorType::Rgba8,
                image::ImageFormat::Png,
            )?;
            rename(&staged, &icon)?;
        }
        let path = self.applications.join("natizap.desktop");
        let entry = entry(self.image.as_deref(), &icon);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(entry.as_str()) {
            create_dir(&self.applications)?;
            // Desktops watch this folder: renaming a whole file in keeps
            // them from reading one half written.
            let staged = self.applications.join(".natizap.desktop.part");
            std::fs::write(&staged, entry).map_err(|source| LauncherError::Write {
                path: staged.clone(),
                source,
            })?;
            rename(&staged, &path)?;
        }
        self.forget_other_icons(&icon);
        Ok(())
    }

    /// Deletes the icons drawn for other themes, now that the entry no
    /// longer points at them.
    fn forget_other_icons(&self, kept: &Path) {
        let Ok(entries) = std::fs::read_dir(&self.icons) else {
            return;
        };
        for path in entries.flatten().map(|entry| entry.path()) {
            let drawn = path.extension().is_some_and(|extension| extension == "png")
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("natizap-"));
            if drawn
                && path != kept
                && let Err(error) = std::fs::remove_file(&path)
            {
                tracing::warn!(%error, path = %path.display(), "could not delete an old icon");
            }
        }
    }
}

/// The package's desktop entry showing `icon`, and starting the AppImage
/// `image` if natizap runs from one.
///
/// Without an image the package's command stays: a build run from
/// elsewhere must not take the installed natizap's place in the launcher,
/// and its `TryExec` hides the entry once no natizap is installed.
fn entry(image: Option<&Path>, icon: &Path) -> String {
    let mut entry =
        String::from("# Written by natizap so the dock shows its icon in the theme in use.\n");
    for line in ENTRY.lines() {
        if let (Some(image), true) = (image, line.starts_with("Exec=")) {
            entry.push_str("Exec=");
            entry.push_str(&exec_argument(image));
        } else if let (Some(image), true) = (image, line.starts_with("TryExec=")) {
            entry.push_str("TryExec=");
            entry.push_str(&escape(&image.to_string_lossy()));
        } else if line.starts_with("Icon=") {
            entry.push_str("Icon=");
            entry.push_str(&escape(&icon.to_string_lossy()));
        } else {
            entry.push_str(line);
        }
        entry.push('\n');
    }
    entry
}

/// `path` as the program of an `Exec` key: quoted, with the characters the
/// Desktop Entry Specification reserves inside quotes escaped, then
/// escaped again as a string value.
fn exec_argument(path: &Path) -> String {
    let mut quoted = String::from("\"");
    for c in path.to_string_lossy().chars() {
        match c {
            '"' | '`' | '$' => {
                quoted.push_str("\\\\");
                quoted.push(c);
            }
            '\\' => quoted.push_str("\\\\\\\\"),
            '%' => quoted.push_str("%%"),
            _ => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

/// `value` as a desktop entry string value.
fn escape(value: &str) -> String {
    value.replace('\\', "\\\\")
}

fn create_dir(dir: &Path) -> Result<(), LauncherError> {
    std::fs::create_dir_all(dir).map_err(|source| LauncherError::Write {
        path: dir.to_owned(),
        source,
    })
}

fn rename(from: &Path, to: &Path) -> Result<(), LauncherError> {
    std::fs::rename(from, to).map_err(|source| LauncherError::Write {
        path: to.to_owned(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use natizap_ui::theme::NATIVE;

    /// A folder of its own for one test.
    fn scratch(test: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("natizap-launcher-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn launcher(dir: &Path) -> Launcher {
        Launcher {
            icons: dir.join("icons"),
            applications: dir.join("applications"),
            image: None,
        }
    }

    type Outcome = Result<(), Box<dyn std::error::Error>>;

    fn icon_line(dir: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
        std::fs::read_to_string(dir.join("applications/natizap.desktop"))?
            .lines()
            .find_map(|line| line.strip_prefix("Icon="))
            .map(PathBuf::from)
            .ok_or_else(|| "the entry has no icon".into())
    }

    #[test]
    fn points_the_entry_at_an_icon_in_the_theme() -> Outcome {
        let dir = scratch("theme");
        let launcher = launcher(&dir);
        let (first, second) = (NATIVE[0].1, NATIVE[1].1);
        launcher.write(first)?;
        let first_icon = icon_line(&dir)?;
        let drawn = image::open(&first_icon)?.into_rgba8();
        assert_eq!(drawn.dimensions(), (SIDE, SIDE));
        assert_eq!(drawn.into_raw(), app_icon(first, SIDE));

        launcher.write(second)?;
        let second_icon = icon_line(&dir)?;
        assert_ne!(first_icon, second_icon, "a new theme needs a new path");
        assert!(second_icon.exists());
        assert!(!first_icon.exists(), "the old icon goes");
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn keeps_the_package_entry_but_its_icon() {
        let entry = entry(None, Path::new("/icons/natizap-1.png"));
        assert!(entry.contains("\nExec=natizap\n"));
        assert!(entry.contains("\nTryExec=natizap\n"));
        assert!(entry.contains("\nIcon=/icons/natizap-1.png\n"));
        assert!(entry.contains("\nStartupWMClass=natizap\n"));
        assert_eq!(entry.matches("Icon=").count(), 1);
    }

    #[test]
    fn starts_the_appimage_natizap_runs_from() {
        let entry = entry(
            Some(Path::new("/home/me/Apps/natizap 1.AppImage")),
            Path::new("/icons/natizap-1.png"),
        );
        assert!(entry.contains("\nExec=\"/home/me/Apps/natizap 1.AppImage\"\n"));
        assert!(entry.contains("\nTryExec=/home/me/Apps/natizap 1.AppImage\n"));
        assert_eq!(entry.matches("Exec=natizap").count(), 0);
    }

    #[test]
    fn escapes_reserved_characters_in_the_command() {
        assert_eq!(exec_argument(Path::new("/a$b/100%")), "\"/a\\\\$b/100%%\"");
    }
}
