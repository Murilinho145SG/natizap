//! Themes the user made, one text file each: the native theme it starts
//! from, then each colour it changes, `key=#rrggbb`, so files written by a
//! newer natizap still read.

use std::fmt::Write as _;

use natizap_ui::theme::{GRAPHITE, NATIVE, Theme};

use crate::tokens;

/// A theme as its file keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ThemeFile {
    /// The native theme it starts from, by name.
    pub(crate) base: String,
    pub(crate) theme: Theme,
}

impl ThemeFile {
    /// Reads `text`. A missing or unknown base starts from Graphite;
    /// unknown keys and unreadable colours are skipped.
    pub(crate) fn parse(text: &str) -> Self {
        let pairs: Vec<(&str, &str)> = text
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.trim(), value.trim()))
            .collect();
        let base = pairs
            .iter()
            .find(|(key, _)| *key == "base")
            .map_or("Graphite", |(_, value)| *value);
        let (base, mut theme) = NATIVE
            .iter()
            .find(|(name, _)| *name == base)
            .map_or(("Graphite", GRAPHITE), |(name, theme)| (*name, *theme));
        for (key, value) in pairs {
            if let Some(token) = tokens::all().find(|token| token.key == key)
                && let Some(color) = token.read(value)
            {
                (token.set)(&mut theme, color);
            }
        }
        Self {
            base: base.to_owned(),
            theme,
        }
    }

    /// The file's text: its base, then every colour.
    pub(crate) fn format(&self) -> String {
        let mut text = format!("base={}\n", self.base);
        for token in tokens::all() {
            let _ = writeln!(
                text,
                "{}={}",
                token.key,
                token.write((token.get)(&self.theme))
            );
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use natizap_ui::theme::MIDNIGHT;

    use super::*;

    #[test]
    fn reads_back_what_it_writes() {
        let mut theme = MIDNIGHT;
        theme.accent = 0x12_34_56;
        theme.hover = 0xab_cd_ef_10;
        let file = ThemeFile {
            base: "Midnight".into(),
            theme,
        };
        assert_eq!(ThemeFile::parse(&file.format()), file);
    }

    #[test]
    fn starts_from_its_base_and_skips_what_it_cannot_read() {
        let file = ThemeFile::parse("base=Midnight\naccent=#00ff00\nlink=oops\nnewer=#123456\n");
        assert_eq!(file.theme.accent, 0x00_ff_00);
        assert_eq!(file.theme.link, MIDNIGHT.link);
        assert_eq!(ThemeFile::parse("").base, "Graphite");
    }
}
