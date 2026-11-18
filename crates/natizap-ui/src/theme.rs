//! Color and type tokens. Colors are `0xRRGGBB`, for use with `gpui::rgb`.

/// Behind everything: the title bar and window edges.
pub const DEEP: u32 = 0x09_0a_0c;
/// The chat list column.
pub const PANEL: u32 = 0x0f_11_14;
/// The conversation pane and full-window screens.
pub const PANE: u32 = 0x0c_0e_11;
/// Hovered and selected surfaces, and messages from others.
pub const RAISED: u32 = 0x1a_1d_22;
/// Hairlines between regions.
pub const LINE: u32 = 0x24_28_2e;
/// Primary text.
pub const TEXT: u32 = 0xec_ee_f1;
/// Secondary text: previews, timestamps, hints.
pub const MUTED: u32 = 0x8b_91_9a;
/// The accent: unread counts and primary actions.
pub const ACCENT: u32 = 0xff_7a_1a;
/// Text and icons drawn on [`ACCENT`].
pub const ACCENT_INK: u32 = 0x16_0b_03;
/// Healthy state, e.g. connected.
pub const MINT: u32 = 0x5f_cf_9f;
/// Something needs attention, e.g. the close button on hover.
pub const ALERT: u32 = 0xd0_4f_45;

/// Messages this account sent: graphite warmed by the accent.
pub const OUTGOING: u32 = 0x26_1d_17;
/// Darkens a picture under text or icons drawn on it, `0xRRGGBBAA` for
/// `gpui::rgba`.
pub const SCRIM: u32 = 0x00_00_00_80;
/// The light rim of glass surfaces, `0xRRGGBBAA` for `gpui::rgba`.
pub const EDGE: u32 = 0xff_ff_ff_14;
/// A faint light over a row under the pointer, `0xRRGGBBAA` for
/// `gpui::rgba`: see-through, so what the row holds stays distinct.
pub const HOVER: u32 = 0xff_ff_ff_0d;
/// Selected text: the accent, faint, `0xRRGGBBAA` for `gpui::rgba`.
pub const SELECTION: u32 = 0xff_7a_1a_59;
/// Links in messages: the accent, lightened to read as text.
pub const LINK: u32 = 0xff_a8_6b;

/// Code: keywords such as `fn`, `if` and `return`.
pub const CODE_KEYWORD: u32 = 0xff_9e_5e;
/// Code: quoted text.
pub const CODE_STRING: u32 = 0xa8_d1_8a;
/// Code: numbers.
pub const CODE_NUMBER: u32 = 0xe6_c0_7b;
/// Code: type names, which start with a capital.
pub const CODE_TYPE: u32 = 0x8f_b4_d6;
/// Code: names being called.
pub const CODE_CALL: u32 = 0xc7_9b_d0;

/// Styles `element` as a glass surface floating over the window: a faint
/// light rim and a soft shadow. Backdrop blur is left out on purpose: GPUI
/// cannot blur what lies behind an element, and on GNOME not even behind
/// the window.
pub fn glass<E: gpui::Styled>(element: E) -> E {
    element
        .border_1()
        .border_color(gpui::rgba(EDGE))
        .shadow_md()
}

/// Avatar backgrounds; muted so names stay the brightest thing in a row.
pub const AVATAR_TINTS: [u32; 6] = [
    0x8a_6a_2f, 0x2f_7a_6f, 0x6d_4a_72, 0x46_61_7a, 0x8a_4b_57, 0x5b_6e_3a,
];
/// Names in group messages: the avatar tints, lightened to read as text.
pub const NAME_TINTS: [u32; 6] = [
    0xe0_b4_6a, 0x6f_d1_c0, 0xc7_9b_d0, 0x8f_b4_d6, 0xe3_9a_a8, 0xb4_c9_7a,
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
