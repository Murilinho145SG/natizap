//! A block of code: monospaced on a darker well, its syntax colored.

use std::ops::Range;

use gpui::prelude::*;
use gpui::{Div, FontStyle, HighlightStyle, SharedString, StyledText, div, px, rgb};

use crate::theme::{
    CODE_CALL, CODE_KEYWORD, CODE_NUMBER, CODE_STRING, CODE_TYPE, DEEP, LINE, MONO_FONT, MUTED,
};

/// `code`, monospaced and colored. Messages rarely say which language they
/// hold, so the coloring reads what most languages share rather than one
/// grammar.
#[must_use]
pub fn code_block(code: impl Into<SharedString>) -> Div {
    let code = code.into();
    let highlights: Vec<(Range<usize>, HighlightStyle)> = highlight(&code)
        .into_iter()
        .map(|(range, token)| (range, token.style()))
        .collect();
    div()
        .font_family(MONO_FONT)
        .text_size(px(12.5))
        .line_height(px(18.0))
        .px(px(10.0))
        .py(px(8.0))
        .rounded(px(8.0))
        .bg(rgb(DEEP))
        .border_1()
        .border_color(rgb(LINE))
        .child(StyledText::new(code).with_highlights(highlights))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
    Comment,
    Quoted,
    Number,
    Keyword,
    Type,
    Call,
}

impl Token {
    fn style(self) -> HighlightStyle {
        let color = match self {
            Self::Comment => MUTED,
            Self::Quoted => CODE_STRING,
            Self::Number => CODE_NUMBER,
            Self::Keyword => CODE_KEYWORD,
            Self::Type => CODE_TYPE,
            Self::Call => CODE_CALL,
        };
        HighlightStyle {
            color: Some(rgb(color).into()),
            font_style: (self == Self::Comment).then_some(FontStyle::Italic),
            ..Default::default()
        }
    }
}

/// Keywords of the languages people paste most: Rust, JavaScript and
/// TypeScript, Python, Go, Java, C and C#, Kotlin, Swift, PHP, SQL and shell.
const KEYWORDS: &[&str] = &[
    "and",
    "as",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "def",
    "default",
    "defer",
    "del",
    "do",
    "elif",
    "else",
    "enum",
    "except",
    "export",
    "extends",
    "false",
    "fi",
    "finally",
    "fn",
    "for",
    "from",
    "func",
    "function",
    "go",
    "if",
    "impl",
    "implements",
    "import",
    "in",
    "interface",
    "is",
    "lambda",
    "let",
    "loop",
    "match",
    "mod",
    "mut",
    "new",
    "nil",
    "none",
    "not",
    "null",
    "of",
    "or",
    "package",
    "pass",
    "private",
    "protected",
    "pub",
    "public",
    "raise",
    "ref",
    "return",
    "select",
    "self",
    "static",
    "struct",
    "super",
    "switch",
    "then",
    "this",
    "throw",
    "trait",
    "true",
    "try",
    "type",
    "typeof",
    "use",
    "val",
    "var",
    "void",
    "where",
    "while",
    "with",
    "yield",
    "None",
    "True",
    "False",
    "SELECT",
    "FROM",
    "WHERE",
    "INSERT",
    "UPDATE",
    "DELETE",
    "JOIN",
    "ORDER",
    "GROUP",
    "BY",
    "INTO",
    "VALUES",
];

/// The colored ranges of `code`, as byte ranges.
fn highlight(code: &str) -> Vec<(Range<usize>, Token)> {
    let chars: Vec<(usize, char)> = code.char_indices().collect();
    let byte = |index: usize| chars.get(index).map_or(code.len(), |&(byte, _)| byte);
    let at = |index: usize| chars.get(index).map(|&(_, c)| c);
    let line_end = |from: usize| {
        (from..chars.len())
            .find(|&i| chars[i].1 == '\n')
            .unwrap_or(chars.len())
    };
    let mut found = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i].1;
        let next = at(i + 1);
        let after_word = i > 0 && is_word(chars[i - 1].1);
        let span = if (c == '/' && next == Some('/'))
            || (c == '#' && next.is_none_or(|n| n == ' ' || n == '\n'))
            || (c == '-' && next == Some('-') && at(i + 2) == Some(' '))
        {
            Some((line_end(i), Token::Comment))
        } else if c == '/' && next == Some('*') {
            let close = (i + 2..chars.len().saturating_sub(1))
                .find(|&k| chars[k].1 == '*' && chars[k + 1].1 == '/')
                .map_or(chars.len(), |k| k + 2);
            Some((close, Token::Comment))
        } else if matches!(c, '"' | '\'' | '`') && !after_word {
            closing_quote(&chars, i)
                .filter(|&end| at(end).is_none_or(|n| !is_word(n)))
                .map(|end| (end, Token::Quoted))
        } else if c.is_ascii_digit() && !after_word {
            let end = (i..chars.len())
                .find(|&k| {
                    !(is_word(chars[k].1)
                        || (chars[k].1 == '.' && at(k + 1).is_some_and(|n| n.is_ascii_digit())))
                })
                .unwrap_or(chars.len());
            Some((end, Token::Number))
        } else if is_word(c) && !after_word {
            let end = (i..chars.len())
                .find(|&k| !is_word(chars[k].1))
                .unwrap_or(chars.len());
            let word = &code[byte(i)..byte(end)];
            // Python's f"…", r"…" and b"…" are text too.
            let prefixed = ["f", "r", "b", "rb", "br", "fr", "rf"].contains(&word)
                && at(end).is_some_and(|n| matches!(n, '"' | '\''));
            if let Some(close) = prefixed.then(|| closing_quote(&chars, end)).flatten() {
                found.push((byte(i)..byte(close), Token::Quoted));
                i = close;
                continue;
            }
            let called = (end..chars.len())
                .find(|&k| chars[k].1 != ' ')
                .is_some_and(|k| chars[k].1 == '(');
            let token = if KEYWORDS.contains(&word) {
                Some(Token::Keyword)
            } else if called {
                Some(Token::Call)
            } else if word.starts_with(|first: char| first.is_uppercase()) {
                Some(Token::Type)
            } else {
                None
            };
            let Some(token) = token else {
                i = end;
                continue;
            };
            Some((end, token))
        } else {
            None
        };
        match span {
            Some((end, token)) => {
                found.push((byte(i)..byte(end), token));
                i = end;
            }
            None => i += 1,
        }
    }
    found
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Where the quote opened at `chars[open]` ends, just past its closing
/// mark. Single and double quotes close on their own line or not at all;
/// with the caller's word checks, a stray apostrophe (Rust's `'a`, "don't")
/// colors nothing.
fn closing_quote(chars: &[(usize, char)], open: usize) -> Option<usize> {
    let quote = chars[open].1;
    let mut k = open + 1;
    while let Some(&(_, c)) = chars.get(k) {
        if c == '\\' {
            k += 2;
            continue;
        }
        if c == quote {
            return Some(k + 1);
        }
        if c == '\n' && quote != '`' {
            return None;
        }
        k += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(code: &str) -> Vec<(&str, Token)> {
        highlight(code)
            .into_iter()
            .map(|(range, token)| (&code[range], token))
            .collect()
    }

    #[test]
    fn colors_what_most_languages_share() {
        assert_eq!(
            tokens("let total = sum(items) + 1.5; // done"),
            [
                ("let", Token::Keyword),
                ("sum", Token::Call),
                ("1.5", Token::Number),
                ("// done", Token::Comment),
            ]
        );
        assert_eq!(
            tokens("def greet(name):\n    print(f\"hi {name}\")  # say it"),
            [
                ("def", Token::Keyword),
                ("greet", Token::Call),
                ("print", Token::Call),
                ("f\"hi {name}\"", Token::Quoted),
                ("# say it", Token::Comment),
            ]
        );
    }

    #[test]
    fn reads_code_that_ends_in_a_word() {
        assert_eq!(tokens("cargo test --workspace"), []);
    }

    #[test]
    fn leaves_lone_apostrophes_and_names_with_digits_alone() {
        assert_eq!(
            tokens("fn f<'a>(x: &'a Vec2) {}"),
            [("fn", Token::Keyword), ("Vec2", Token::Type)]
        );
        assert_eq!(
            tokens("x = 'it' + \"don't\""),
            [("'it'", Token::Quoted), ("\"don't\"", Token::Quoted)]
        );
    }
}
