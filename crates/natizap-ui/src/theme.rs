//! Color and type tokens. Colors are `0xRRGGBB`, for use with `gpui::rgb`.

/// Behind everything: the title bar and window edges.
pub const DEEP: u32 = 0x0e_17_15;
/// The chat list column.
pub const PANEL: u32 = 0x12_1f_1c;
/// The conversation pane and full-window screens.
pub const PANE: u32 = 0x16_26_22;
/// Hovered and selected surfaces.
pub const RAISED: u32 = 0x1e_33_2e;
/// Hairlines between regions.
pub const LINE: u32 = 0x22_3a_34;
/// Primary text.
pub const TEXT: u32 = 0xe6_ef_ec;
/// Secondary text: previews, timestamps, hints.
pub const MUTED: u32 = 0x8b_a3_9c;
/// The accent: unread counts and primary actions.
pub const HONEY: u32 = 0xf2_b8_4b;
/// Text drawn on [`HONEY`].
pub const HONEY_INK: u32 = 0x2a_1d_05;
/// Healthy state, e.g. connected.
pub const MINT: u32 = 0x5f_cf_9f;
/// Something needs attention, e.g. the close button on hover.
pub const ALERT: u32 = 0xd0_4f_45;

/// Avatar backgrounds; muted so names stay the brightest thing in a row.
pub const AVATAR_TINTS: [u32; 6] = [
    0x8a_6a_2f, 0x2f_7a_6f, 0x6d_4a_72, 0x46_61_7a, 0x8a_4b_57, 0x5b_6e_3a,
];

/// Interface text.
pub const UI_FONT: &str = "Figtree";
/// The wordmark and headings only.
pub const DISPLAY_FONT: &str = "Bricolage Grotesque";
