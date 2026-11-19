//! The look: a [`Theme`] of colour tokens, kept as a GPUI global and read
//! with [`ActiveTheme::theme`], the native themes, and the fonts. Colours
//! are `0xRRGGBB` for `gpui::rgb`, or `0xRRGGBBAA` for `gpui::rgba` where
//! a token says so.

use gpui::{App, Global, Styled};

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

/// Near-black graphite with a single orange accent: natizap's own look.
pub const GRAPHITE: Theme = Theme {
    deep: 0x09_0a_0c,
    panel: 0x0f_11_14,
    pane: 0x0c_0e_11,
    raised: 0x1a_1d_22,
    line: 0x24_28_2e,
    text: 0xec_ee_f1,
    muted: 0x8b_91_9a,
    accent: 0xff_7a_1a,
    accent_ink: 0x16_0b_03,
    mint: 0x5f_cf_9f,
    alert: 0xd0_4f_45,
    outgoing: 0x26_1d_17,
    scrim: 0x00_00_00_80,
    edge: 0xff_ff_ff_14,
    thumb: 0xff_ff_ff_26,
    thumb_active: 0xff_ff_ff_4d,
    hover: 0xff_ff_ff_0d,
    selection: 0xff_7a_1a_59,
    link: 0xff_a8_6b,
    code_keyword: 0xff_9e_5e,
    code_string: 0xa8_d1_8a,
    code_number: 0xe6_c0_7b,
    code_type: 0x8f_b4_d6,
    code_call: 0xc7_9b_d0,
    name_tints: [
        0xe0_b4_6a, 0x6f_d1_c0, 0xc7_9b_d0, 0x8f_b4_d6, 0xe3_9a_a8, 0xb4_c9_7a,
    ],
};

/// Blue-black night with a cool blue accent.
pub const MIDNIGHT: Theme = Theme {
    deep: 0x06_09_10,
    panel: 0x0b_10_1a,
    pane: 0x09_0d_17,
    raised: 0x15_1c_2a,
    line: 0x1e_27_37,
    text: 0xe7_ec_f5,
    muted: 0x86_91_a7,
    accent: 0x4c_8d_ff,
    accent_ink: 0x03_0d_1f,
    mint: 0x5f_cf_9f,
    alert: 0xe0_5a_55,
    outgoing: 0x14_23_3f,
    scrim: 0x00_00_00_80,
    edge: 0xff_ff_ff_14,
    thumb: 0xff_ff_ff_26,
    thumb_active: 0xff_ff_ff_4d,
    hover: 0xff_ff_ff_0d,
    selection: 0x4c_8d_ff_59,
    link: 0x8f_b8_ff,
    code_keyword: 0x8f_b8_ff,
    code_string: 0xa8_d1_8a,
    code_number: 0xe6_c0_7b,
    code_type: 0x7f_d4_d0,
    code_call: 0xc7_9b_d0,
    name_tints: [
        0xe0_b4_6a, 0x6f_d1_c0, 0xc7_9b_d0, 0x8f_b4_d6, 0xe3_9a_a8, 0xb4_c9_7a,
    ],
};

/// True black for OLED screens, with the orange accent.
pub const OLED: Theme = Theme {
    deep: 0x00_00_00,
    panel: 0x00_00_00,
    pane: 0x00_00_00,
    raised: 0x12_12_12,
    line: 0x1c_1c_1c,
    text: 0xee_ee_ee,
    muted: 0x88_88_88,
    accent: 0xff_7a_1a,
    accent_ink: 0x16_0b_03,
    mint: 0x5f_cf_9f,
    alert: 0xd0_4f_45,
    outgoing: 0x1f_16_0f,
    scrim: 0x00_00_00_99,
    edge: 0xff_ff_ff_1a,
    thumb: 0xff_ff_ff_26,
    thumb_active: 0xff_ff_ff_4d,
    hover: 0xff_ff_ff_0f,
    selection: 0xff_7a_1a_59,
    link: 0xff_a8_6b,
    code_keyword: 0xff_9e_5e,
    code_string: 0xa8_d1_8a,
    code_number: 0xe6_c0_7b,
    code_type: 0x8f_b4_d6,
    code_call: 0xc7_9b_d0,
    name_tints: [
        0xe0_b4_6a, 0x6f_d1_c0, 0xc7_9b_d0, 0x8f_b4_d6, 0xe3_9a_a8, 0xb4_c9_7a,
    ],
};

/// Light paper with the orange accent, deepened to read on white.
pub const LIGHT: Theme = Theme {
    deep: 0xe6_e8_ec,
    panel: 0xf5_f6_f8,
    pane: 0xff_ff_ff,
    raised: 0xea_ec_f0,
    line: 0xdc_df_e5,
    text: 0x15_17_1a,
    muted: 0x67_6e_7a,
    accent: 0xe8_62_05,
    accent_ink: 0xff_ff_ff,
    mint: 0x1a_8f_5f,
    alert: 0xc2_37_2c,
    outgoing: 0xfd_e6_d3,
    scrim: 0x00_00_00_66,
    edge: 0x00_00_00_1a,
    thumb: 0x00_00_00_26,
    thumb_active: 0x00_00_00_4d,
    hover: 0x00_00_00_0a,
    selection: 0xe8_62_05_40,
    link: 0xc4_4f_05,
    code_keyword: 0xb1_42_0c,
    code_string: 0x3a_77_28,
    code_number: 0x95_66_00,
    code_type: 0x2a_5b_88,
    code_call: 0x7a_3a_8c,
    name_tints: [
        0x9a_62_00, 0x16_7f_70, 0x7e_3f_8a, 0x2b_5f_92, 0xa8_3a_55, 0x4f_76_1c,
    ],
};

/// The themes natizap comes with, by name, its own first.
pub const NATIVE: [(&str, Theme); 4] = [
    ("Graphite", GRAPHITE),
    ("Midnight", MIDNIGHT),
    ("OLED", OLED),
    ("Light", LIGHT),
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
        }
    }
}
