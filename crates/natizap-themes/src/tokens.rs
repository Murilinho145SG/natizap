//! The colours a theme is made of, as the editor groups them and as theme
//! files name them.

use natizap_ui::theme::Theme;

/// One colour of a theme.
#[derive(Clone, Copy)]
pub(crate) struct Token {
    /// How theme files name it.
    pub(crate) key: &'static str,
    /// How the editor labels it.
    pub(crate) label: &'static str,
    /// Whether it is see-through, written `#rrggbbaa`.
    pub(crate) alpha: bool,
    pub(crate) get: fn(&Theme) -> u32,
    pub(crate) set: fn(&mut Theme, u32),
}

macro_rules! token {
    ($field:ident, $label:literal, $alpha:literal) => {
        Token {
            key: stringify!($field),
            label: $label,
            alpha: $alpha,
            get: |theme| theme.$field,
            set: |theme, color| theme.$field = color,
        }
    };
}

/// Every colour a theme file or the editor may change, by group.
pub(crate) const GROUPS: &[(&str, &[Token])] = &[
    (
        "Surfaces",
        &[
            token!(deep, "Window edge", false),
            token!(panel, "Chat list", false),
            token!(pane, "Conversation", false),
            token!(raised, "Raised", false),
            token!(line, "Lines", false),
        ],
    ),
    (
        "Text",
        &[
            token!(text, "Text", false),
            token!(muted, "Secondary text", false),
            token!(link, "Links", false),
        ],
    ),
    (
        "Accent",
        &[
            token!(accent, "Accent", false),
            token!(accent_ink, "On the accent", false),
            token!(selection, "Selection", true),
        ],
    ),
    ("Messages", &[token!(outgoing, "Sent messages", false)]),
    (
        "States",
        &[
            token!(mint, "Healthy", false),
            token!(alert, "Attention", false),
        ],
    ),
    (
        "Glass",
        &[
            token!(edge, "Rim", true),
            token!(hover, "Hover", true),
            token!(scrim, "Scrim", true),
            token!(thumb, "Scrollbar", true),
            token!(thumb_active, "Scrollbar, held", true),
        ],
    ),
    (
        "Code",
        &[
            token!(code_keyword, "Keywords", false),
            token!(code_string, "Strings", false),
            token!(code_number, "Numbers", false),
            token!(code_type, "Types", false),
            token!(code_call, "Calls", false),
        ],
    ),
];

/// Every token, in the editor's order.
pub(crate) fn all() -> impl Iterator<Item = &'static Token> {
    GROUPS.iter().flat_map(|(_, tokens)| tokens.iter())
}

impl Token {
    /// `color` as theme files write it: `#rrggbb`, or `#rrggbbaa` when
    /// see-through.
    pub(crate) fn write(self, color: u32) -> String {
        if self.alpha {
            format!("#{color:08x}")
        } else {
            format!("#{color:06x}")
        }
    }

    /// The colour `text` writes, if it writes one this token takes.
    pub(crate) fn read(self, text: &str) -> Option<u32> {
        let hex = text.trim().trim_start_matches('#');
        let digits = if self.alpha { 8 } else { 6 };
        (hex.len() == digits)
            .then(|| u32::from_str_radix(hex, 16).ok())
            .flatten()
    }
}
