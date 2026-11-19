//! Which theme natizap draws with, the ones it comes with and the ones the
//! user made, kept across runs.

use std::path::{Path, PathBuf};

use gpui::{AppContext as _, Context, SharedString};
use natizap_ui::theme::{GRAPHITE, NATIVE, Theme};

use crate::file::ThemeFile;

/// A theme the user can choose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub name: SharedString,
    pub theme: Theme,
    /// The user made it, so it can be changed and deleted.
    pub custom: bool,
}

/// Every theme, and the one in use, applied as the app's theme.
#[derive(Debug)]
pub struct ThemeBook {
    /// The themes the user made, by name, sorted.
    custom: Vec<(SharedString, ThemeFile)>,
    chosen: SharedString,
    /// Where the choice is kept.
    chosen_path: PathBuf,
    /// Where the user's themes are kept, one file each.
    dir: PathBuf,
}

impl ThemeBook {
    /// Reads the themes kept in `data_dir` and draws with the chosen one.
    pub fn new(data_dir: &Path, cx: &mut Context<Self>) -> Self {
        let dir = data_dir.join("themes");
        let chosen_path = data_dir.join("themes.conf");
        let custom = read_custom(&dir);
        let chosen = std::fs::read_to_string(&chosen_path)
            .ok()
            .and_then(|text| {
                text.lines()
                    .filter_map(|line| line.split_once('='))
                    .find(|(key, _)| key.trim() == "theme")
                    .map(|(_, name)| SharedString::from(name.trim().to_owned()))
            })
            .unwrap_or_else(|| "Graphite".into());
        let book = Self {
            custom,
            chosen,
            chosen_path,
            dir,
        };
        cx.set_global(book.theme_of(&book.chosen).unwrap_or(GRAPHITE));
        book
    }

    /// Every theme, the native ones first.
    #[must_use]
    pub fn choices(&self) -> Vec<Choice> {
        NATIVE
            .iter()
            .map(|(name, theme)| Choice {
                name: (*name).into(),
                theme: *theme,
                custom: false,
            })
            .chain(self.custom.iter().map(|(name, file)| Choice {
                name: name.clone(),
                theme: file.theme,
                custom: true,
            }))
            .collect()
    }

    /// The theme in use, by name.
    #[must_use]
    pub fn chosen(&self) -> &SharedString {
        &self.chosen
    }

    /// The native theme a user's theme starts from, or the theme itself
    /// when native.
    pub(crate) fn base_of(&self, name: &str) -> SharedString {
        self.custom
            .iter()
            .find(|(custom, _)| custom == name)
            .map_or_else(
                || name.to_owned().into(),
                |(_, file)| file.base.clone().into(),
            )
    }

    fn theme_of(&self, name: &str) -> Option<Theme> {
        self.choices()
            .into_iter()
            .find(|choice| choice.name == name)
            .map(|choice| choice.theme)
    }

    /// Draws with the theme called `name` from now on, and keeps the choice.
    pub fn choose(&mut self, name: &SharedString, cx: &mut Context<Self>) {
        let Some(theme) = self.theme_of(name) else {
            return;
        };
        self.chosen = name.clone();
        Self::apply(theme, cx);
        let (path, text) = (self.chosen_path.clone(), format!("theme={name}\n"));
        cx.background_spawn(async move {
            if let Err(error) = std::fs::write(&path, text) {
                tracing::warn!(%error, "could not keep the chosen theme");
            }
        })
        .detach();
        cx.notify();
    }

    /// Draws with `theme` for now, without choosing it: the editor's live
    /// preview.
    pub(crate) fn preview(theme: Theme, cx: &mut Context<Self>) {
        Self::apply(theme, cx);
    }

    /// Draws with the chosen theme again, after a preview.
    pub(crate) fn restore(&self, cx: &mut Context<Self>) {
        Self::apply(self.theme_of(&self.chosen).unwrap_or(GRAPHITE), cx);
    }

    /// Keeps `theme`, starting from the native `base`, as the user's theme
    /// called `name`, and draws with it.
    pub(crate) fn save(&mut self, name: &str, base: &str, theme: Theme, cx: &mut Context<Self>) {
        let name: SharedString = clean_name(name).into();
        let file = ThemeFile {
            base: base.to_owned(),
            theme,
        };
        let (path, text) = (self.dir.join(format!("{name}.theme")), file.format());
        let dir = self.dir.clone();
        cx.background_spawn(async move {
            let written = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, text));
            if let Err(error) = written {
                tracing::warn!(%error, "could not keep a theme");
            }
        })
        .detach();
        self.custom.retain(|(custom, _)| *custom != name);
        self.custom.push((name.clone(), file));
        self.custom.sort_by(|a, b| a.0.cmp(&b.0));
        self.choose(&name, cx);
    }

    /// Deletes the user's theme called `name`; Graphite takes its place if
    /// it was in use.
    pub(crate) fn delete(&mut self, name: &SharedString, cx: &mut Context<Self>) {
        self.custom.retain(|(custom, _)| custom != name);
        let path = self.dir.join(format!("{name}.theme"));
        cx.background_spawn(async move {
            if let Err(error) = std::fs::remove_file(&path) {
                tracing::warn!(%error, "could not delete a theme");
            }
        })
        .detach();
        if self.chosen == *name {
            self.choose(&"Graphite".into(), cx);
        }
        cx.notify();
    }

    fn apply(theme: Theme, cx: &mut Context<Self>) {
        cx.set_global(theme);
        cx.refresh_windows();
    }
}

/// The user's themes in `dir`, by name, sorted.
fn read_custom(dir: &Path) -> Vec<(SharedString, ThemeFile)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut custom: Vec<(SharedString, ThemeFile)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "theme")
        })
        .filter_map(|path| {
            let name = path.file_stem()?.to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&path).ok()?;
            Some((name.into(), ThemeFile::parse(&text)))
        })
        .collect();
    custom.sort_by(|a, b| a.0.cmp(&b.0));
    custom
}

/// `name` fit for a file name: letters, digits, spaces, dashes and
/// underscores, never a native theme's name.
fn clean_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        .collect::<String>()
        .trim()
        .to_owned();
    let cleaned = if cleaned.is_empty() {
        "My theme".to_owned()
    } else {
        cleaned
    };
    if NATIVE.iter().any(|(native, _)| *native == cleaned) {
        format!("{cleaned} copy")
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_fit_for_files_and_never_native() {
        assert_eq!(clean_name("Sun/set ✨"), "Sunset");
        assert_eq!(clean_name("  "), "My theme");
        assert_eq!(clean_name("Midnight"), "Midnight copy");
    }
}
