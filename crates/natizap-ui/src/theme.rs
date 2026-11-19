//! The look: a [`Theme`] of colour tokens, kept as a GPUI global and read
//! with [`ActiveTheme::theme`], the native themes, and the fonts. Colours
//! are `0xRRGGBB` for `gpui::rgb`, or `0xRRGGBBAA` for `gpui::rgba` where
//! a token says so.

use gpui::{App, Global, Styled};

mod dark;
mod light;

pub use dark::{
    CATPPUCCIN_FRAPPE, CATPPUCCIN_MACCHIATO, CATPPUCCIN_MOCHA, DRACULA, GITHUB_DARK, GRAPHITE,
    GRUVBOX_DARK, MIDNIGHT, MONOKAI, NORD, OLED, ONE_DARK, ROSE_PINE, SOLARIZED_DARK, TOKYO_NIGHT,
};
pub use light::{
    CATPPUCCIN_LATTE, GITHUB_LIGHT, GRUVBOX_LIGHT, LIGHT, ROSE_PINE_DAWN, SOLARIZED_LIGHT,
};

/// Every colour the app draws with. Copied freely: it is a handful of
/// numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// Behind everything: the title bar and window edges.
    pub deep: u32,
    /// The chat list column.
    pub panel: u32,
    /// The conversation pane and full-window screens.
    pub pane: u32,
    /// Hovered and selected surfaces, and messages from others.
    pub raised: u32,
    /// Hairlines between regions.
    pub line: u32,
    /// Primary text.
    pub text: u32,
    /// Secondary text: previews, timestamps, hints.
    pub muted: u32,
    /// The accent: unread counts and primary actions.
    pub accent: u32,
    /// Text and icons drawn on the accent.
    pub accent_ink: u32,
    /// Healthy state, e.g. connected.
    pub mint: u32,
    /// Something needs attention, e.g. the close button on hover.
    pub alert: u32,
    /// Messages this account sent.
    pub outgoing: u32,
    /// Darkens a picture under text or icons drawn on it; `0xRRGGBBAA`.
    pub scrim: u32,
    /// The rim of glass surfaces; `0xRRGGBBAA`.
    pub edge: u32,
    /// A scrollbar's thumb at rest; `0xRRGGBBAA`.
    pub thumb: u32,
    /// A scrollbar's thumb under the pointer or dragged; `0xRRGGBBAA`.
    pub thumb_active: u32,
    /// A faint tint over a row under the pointer, see-through so what the
    /// row holds stays distinct; `0xRRGGBBAA`.
    pub hover: u32,
    /// Selected text: the accent, faint; `0xRRGGBBAA`.
    pub selection: u32,
    /// Links in messages.
    pub link: u32,
    /// Code: keywords such as `fn`, `if` and `return`.
    pub code_keyword: u32,
    /// Code: quoted text.
    pub code_string: u32,
    /// Code: numbers.
    pub code_number: u32,
    /// Code: type names, which start with a capital.
    pub code_type: u32,
    /// Code: names being called.
    pub code_call: u32,
    /// Names in group messages, one per person, picked by
    /// [`crate::name_color`].
    pub name_tints: [u32; 6],
}

impl Global for Theme {}

impl Theme {
    /// Styles `element` as a glass surface floating over the window: a
    /// faint rim and a soft shadow. Backdrop blur is left out on purpose:
    /// GPUI cannot blur what lies behind an element, and on GNOME not even
    /// behind the window.
    pub fn glass<E: Styled>(&self, element: E) -> E {
        element
            .border_1()
            .border_color(gpui::rgba(self.edge))
            .shadow_md()
    }
}

/// The theme in use, wherever there is a context.
pub trait ActiveTheme {
    /// The theme the app draws with now.
    fn theme(&self) -> &Theme;
}

impl ActiveTheme for App {
    fn theme(&self) -> &Theme {
        self.try_global::<Theme>().unwrap_or(&GRAPHITE)
    }
}

/// The themes natizap comes with, by name: its own first, then the ports,
/// families together.
pub const NATIVE: [(&str, Theme); 21] = [
    ("Graphite", GRAPHITE),
    ("Midnight", MIDNIGHT),
    ("OLED", OLED),
    ("Light", LIGHT),
    ("Catppuccin Latte", CATPPUCCIN_LATTE),
    ("Catppuccin Frappé", CATPPUCCIN_FRAPPE),
    ("Catppuccin Macchiato", CATPPUCCIN_MACCHIATO),
    ("Catppuccin Mocha", CATPPUCCIN_MOCHA),
    ("Dracula", DRACULA),
    ("GitHub Dark", GITHUB_DARK),
    ("GitHub Light", GITHUB_LIGHT),
    ("Gruvbox Dark", GRUVBOX_DARK),
    ("Gruvbox Light", GRUVBOX_LIGHT),
    ("Monokai", MONOKAI),
    ("Nord", NORD),
    ("One Dark", ONE_DARK),
    ("Rosé Pine", ROSE_PINE),
    ("Rosé Pine Dawn", ROSE_PINE_DAWN),
    ("Solarized Dark", SOLARIZED_DARK),
    ("Solarized Light", SOLARIZED_LIGHT),
    ("Tokyo Night", TOKYO_NIGHT),
];

/// Avatar backgrounds; muted so names stay the brightest thing in a row.
pub const AVATAR_TINTS: [u32; 6] = [
    0x8a_6a_2f, 0x2f_7a_6f, 0x6d_4a_72, 0x46_61_7a, 0x8a_4b_57, 0x5b_6e_3a,
];
/// Backgrounds a text status can be posted on, 0xAARRGGBB as WhatsApp
/// sends them: deep enough for white text to read on each.
pub const STATUS_BACKGROUNDS: [u32; 8] = [
    0xff_c2_5e_1c,
    0xff_1e_6e_4f,
    0xff_2f_4f_8a,
    0xff_6d_3a_7d,
    0xff_8a_2f_45,
    0xff_3d_4a_57,
    0xff_7a_5c_1e,
    0xff_1f_1f_23,
];

/// Interface text.
pub const UI_FONT: &str = "Figtree";
/// The wordmark and headings only.
pub const DISPLAY_FONT: &str = "Bricolage Grotesque";
/// Code, in messages and code blocks.
pub const MONO_FONT: &str = "JetBrains Mono";
/// Color emoji: Noto Color Emoji, adapted so GPUI accepts it as a fallback
/// (see `scripts/build-emoji-font.py`). Without it, emoji that system fonts
/// such as `DejaVu Sans` also cover render monochrome.
pub const EMOJI_FONT: &str = "Natizap Emoji";

/// The app's base font: [`UI_FONT`] with [`EMOJI_FONT`] as fallback. Set it
/// once at the root; children inherit the fallback even when they change the
/// family or weight.
#[must_use]
pub fn base_font() -> gpui::Font {
    gpui::Font {
        fallbacks: Some(gpui::FontFallbacks::from_fonts(vec![EMOJI_FONT.to_owned()])),
        ..gpui::font(UI_FONT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG's relative luminance of a `0xRRGGBB` colour.
    fn luminance(color: u32) -> f64 {
        let channel = |shift: u32| {
            let value = f64::from((color >> shift) & 0xff) / 255.0;
            if value <= 0.039_28 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }

    fn contrast(a: u32, b: u32) -> f64 {
        let (light, dark) = {
            let (a, b) = (luminance(a), luminance(b));
            if a > b { (a, b) } else { (b, a) }
        };
        (light + 0.05) / (dark + 0.05)
    }

    #[test]
    fn every_native_theme_reads() {
        for (name, theme) in NATIVE {
            for surface in [theme.pane, theme.panel, theme.raised] {
                assert!(contrast(theme.text, surface) >= 7.0, "{name}: text");
                assert!(contrast(theme.muted, surface) >= 3.0, "{name}: muted");
            }
            assert!(
                contrast(theme.accent_ink, theme.accent) >= 3.0,
                "{name}: accent ink"
            );
            assert!(
                contrast(theme.text, theme.outgoing) >= 7.0,
                "{name}: sent messages"
            );
            // Names, links and code sit on others' bubbles.
            let tinted = [
                theme.link,
                theme.code_keyword,
                theme.code_string,
                theme.code_number,
                theme.code_type,
                theme.code_call,
            ];
            for color in tinted.into_iter().chain(theme.name_tints) {
                assert!(
                    contrast(color, theme.raised) >= 3.0,
                    "{name}: {color:06x} on bubbles"
                );
            }
        }
    }
}
