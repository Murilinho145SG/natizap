//! WhatsApp's text markup: `*bold*`, `_italic_`, `~strikethrough~`,
//! `` `code` ``, ```` ```code blocks``` ````, `> quotes` and `- lists`.
//! `**bold**` is accepted too, since bots often send Markdown. Links, email
//! addresses and phone numbers are found too, and kept whole: markup
//! symbols inside them are part of the address.

use std::ops::Range;

/// A style applied to part of a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// `*bold*`
    Bold,
    /// `_italic_`
    Italic,
    /// `~strikethrough~`
    Strikethrough,
    /// `` `code` `` within a line.
    Code,
    /// A ```` ``` ```` block of code, on lines of its own.
    CodeBlock,
    /// A line starting with `> `.
    Quote,
    /// A web address, email address or phone number to open.
    Link(LinkKind),
}

/// What a link opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// A web page, `https://…` or `www.…`.
    Web,
    /// An email address.
    Email,
    /// A phone number written with its country code, `+…`.
    Phone,
}

impl LinkKind {
    /// The address to open for the link text `text`.
    #[must_use]
    pub fn target(self, text: &str) -> String {
        match self {
            Self::Web if text.starts_with("www.") => format!("https://{text}"),
            Self::Web => text.to_owned(),
            Self::Email => format!("mailto:{text}"),
            Self::Phone => text.chars().filter(char::is_ascii_digit).collect(),
        }
    }
}

/// Message text with the markup symbols removed and the styles they asked
/// for, as byte ranges into `text`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Marked {
    /// The text to show.
    pub text: String,
    /// Styled ranges of `text`; ranges may nest.
    pub marks: Vec<(Range<usize>, Mark)>,
}

impl Marked {
    /// Reads the markup in `source`.
    #[must_use]
    pub fn parse(source: &str) -> Self {
        let mut marked = Self::default();
        let mut blocks = source.split("```");
        let mut in_code = false;
        while let Some(block) = blocks.next() {
            // An unclosed fence is just text.
            if in_code && blocks.clone().next().is_some() {
                let start = marked.text.len();
                marked.text.push_str(code(block));
                marked
                    .marks
                    .push((start..marked.text.len(), Mark::CodeBlock));
            } else {
                if in_code {
                    marked.text.push_str("```");
                }
                marked.lines(block);
            }
            in_code = !in_code;
        }
        marked
    }

    fn lines(&mut self, block: &str) {
        for (index, line) in block.split('\n').enumerate() {
            if index > 0 {
                self.text.push('\n');
            }
            if let Some(quoted) = line.strip_prefix("> ") {
                let start = self.text.len();
                self.inline(quoted);
                self.marks.push((start..self.text.len(), Mark::Quote));
            } else if let Some(item) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
                self.text.push_str("• ");
                self.inline(item);
            } else {
                self.inline(line);
            }
        }
    }

    fn inline(&mut self, line: &str) {
        let chars: Vec<(usize, char)> = line.char_indices().collect();
        let mut i = 0;
        while i < chars.len() {
            let (_, c) = chars[i];
            if let Some((end, kind)) = link(&chars, i) {
                let start = self.text.len();
                self.text.extend(chars[i..end].iter().map(|&(_, c)| c));
                self.marks.push((start..self.text.len(), Mark::Link(kind)));
                i = end;
                continue;
            }
            if let Some(span) = Span::find(line, &chars, i) {
                let start = self.text.len();
                if span.mark == Mark::Code {
                    self.text.push_str(span.inner);
                } else {
                    self.inline(span.inner);
                }
                self.marks.push((start..self.text.len(), span.mark));
                i = span.end;
                continue;
            }
            self.text.push(c);
            i += 1;
        }
    }
}

/// A link starting at `chars[start]`: where it ends and what it is.
fn link(chars: &[(usize, char)], start: usize) -> Option<(usize, LinkKind)> {
    if start > 0 && chars[start - 1].1.is_alphanumeric() {
        return None;
    }
    let rest: String = chars[start..].iter().map(|&(_, c)| c).take(8).collect();
    let word_end = (start..chars.len())
        .find(|&i| chars[i].1.is_whitespace())
        .unwrap_or(chars.len());
    if ["https://", "http://", "www."]
        .iter()
        .any(|prefix| rest.starts_with(prefix))
    {
        let end = trim_web_end(chars, start, word_end);
        return (end > start + 4).then_some((end, LinkKind::Web));
    }
    if chars[start].1 == '+' {
        return phone(chars, start);
    }
    email(chars, start, word_end)
}

/// Leaves out the punctuation that ends a sentence, and a closing bracket
/// the link did not open.
fn trim_web_end(chars: &[(usize, char)], start: usize, mut end: usize) -> usize {
    loop {
        let Some(&(_, last)) = chars.get(end.wrapping_sub(1)).filter(|_| end > start) else {
            return end;
        };
        let opened = |open: char| chars[start..end].iter().any(|&(_, c)| c == open);
        let dangling = match last {
            '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '"' => true,
            ')' => !opened('('),
            ']' => !opened('['),
            _ => false,
        };
        if !dangling {
            return end;
        }
        end -= 1;
    }
}

/// `+55 11 91234-5678` and the like: 8 to 15 digits, with spaces, dashes
/// and brackets between them.
fn phone(chars: &[(usize, char)], start: usize) -> Option<(usize, LinkKind)> {
    let mut digits = 0;
    let mut end = start + 1;
    let mut last_digit = start;
    while let Some(&(_, c)) = chars.get(end) {
        if c.is_ascii_digit() {
            digits += 1;
            last_digit = end;
        } else if !matches!(c, ' ' | '-' | '(' | ')') {
            break;
        }
        end += 1;
    }
    let after = chars.get(last_digit + 1).map(|&(_, c)| c);
    let whole = after.is_none_or(|c| !c.is_alphanumeric());
    ((8..=15).contains(&digits) && whole).then_some((last_digit + 1, LinkKind::Phone))
}

/// `name@domain.tld`, ending before trailing punctuation.
fn email(chars: &[(usize, char)], start: usize, word_end: usize) -> Option<(usize, LinkKind)> {
    let local = |c: char| c.is_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-');
    let at = (start..word_end).find(|&i| chars[i].1 == '@')?;
    if at == start || !chars[start..at].iter().all(|&(_, c)| local(c)) {
        return None;
    }
    let mut end = at + 1;
    while end < word_end && (chars[end].1.is_alphanumeric() || matches!(chars[end].1, '.' | '-')) {
        end += 1;
    }
    while end > at + 1 && matches!(chars[end - 1].1, '.' | '-') {
        end -= 1;
    }
    let domain: String = chars[at + 1..end].iter().map(|&(_, c)| c).collect();
    let tld = domain.rsplit_once('.').map(|(_, tld)| tld)?;
    (tld.len() >= 2 && tld.chars().all(char::is_alphabetic)).then_some((end, LinkKind::Email))
}

/// The code in a fenced block, without the language named on its opening
/// line (```` ```rust ````), which bots add as in Markdown.
fn code(block: &str) -> &str {
    let named = block.split_once('\n').filter(|(first, _)| {
        !first.is_empty()
            && first
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '#' | '-' | '_'))
    });
    named.map_or(block, |(_, code)| code).trim_matches('\n')
}

struct Span<'a> {
    mark: Mark,
    inner: &'a str,
    /// Index in `chars` just past the closing symbol.
    end: usize,
}

impl<'a> Span<'a> {
    /// A styled span opening at `chars[open]`, if one does.
    fn find(line: &'a str, chars: &[(usize, char)], open: usize) -> Option<Self> {
        let symbol = chars[open].1;
        let mark = match symbol {
            '*' => Mark::Bold,
            '_' => Mark::Italic,
            '~' => Mark::Strikethrough,
            '`' => Mark::Code,
            _ => return None,
        };
        let width = if symbol == '*' && chars.get(open + 1).is_some_and(|&(_, c)| c == '*') {
            2
        } else {
            1
        };
        // Symbols inside words are text: snake_case, 2*3*4.
        let after_word = open > 0 && chars[open - 1].1.is_alphanumeric();
        let first = chars.get(open + width)?.1;
        if after_word || first.is_whitespace() || first == symbol {
            return None;
        }
        let close = (open + width + 1..=chars.len().checked_sub(width)?).find(|&close| {
            (0..width).all(|k| chars[close + k].1 == symbol)
                && !chars[close - 1].1.is_whitespace()
                && chars
                    .get(close + width)
                    .is_none_or(|&(_, c)| !c.is_alphanumeric())
        })?;
        let byte = |index: usize| chars.get(index).map_or(line.len(), |&(byte, _)| byte);
        Some(Self {
            mark,
            inner: &line[byte(open + width)..byte(close)],
            end: close + width,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links(source: &str) -> Vec<(&str, LinkKind)> {
        let marked = Marked::parse(source);
        let text: &'static str = Box::leak(marked.text.into_boxed_str());
        marked
            .marks
            .into_iter()
            .filter_map(|(range, mark)| match mark {
                Mark::Link(kind) => Some((&text[range], kind)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn finds_web_addresses_whole_and_without_trailing_punctuation() {
        assert_eq!(
            links("see https://x.com/a_b_c*d, and (www.site.com.br)."),
            [
                ("https://x.com/a_b_c*d", LinkKind::Web),
                ("www.site.com.br", LinkKind::Web)
            ]
        );
        assert_eq!(
            links("wiki https://en.wikipedia.org/wiki/Rust_(language)"),
            [(
                "https://en.wikipedia.org/wiki/Rust_(language)",
                LinkKind::Web
            )]
        );
    }

    #[test]
    fn finds_emails_and_phone_numbers() {
        assert_eq!(
            links("write to contato@padaria.com.br."),
            [("contato@padaria.com.br", LinkKind::Email)]
        );
        assert_eq!(
            links("call +55 11 91234-5678 now"),
            [("+55 11 91234-5678", LinkKind::Phone)]
        );
        assert_eq!(links("costs +5 or 2024-01-02, @someone"), []);
    }

    #[test]
    fn opens_each_kind_of_link_where_it_belongs() {
        assert_eq!(LinkKind::Web.target("www.site.com"), "https://www.site.com");
        assert_eq!(LinkKind::Email.target("a@b.com"), "mailto:a@b.com");
        assert_eq!(LinkKind::Phone.target("+55 11 91234-5678"), "5511912345678");
    }

    fn styled(source: &str) -> (String, Vec<(String, Mark)>) {
        let marked = Marked::parse(source);
        let marks = marked
            .marks
            .iter()
            .map(|(range, mark)| (marked.text[range.clone()].to_owned(), *mark))
            .collect();
        (marked.text, marks)
    }

    fn marks(expected: &[(&str, Mark)]) -> Vec<(String, Mark)> {
        expected
            .iter()
            .map(|(text, mark)| ((*text).to_owned(), *mark))
            .collect()
    }

    #[test]
    fn removes_symbols_and_styles_their_text() {
        assert_eq!(
            styled("*hi* _you_ ~no~ `x`"),
            (
                "hi you no x".to_owned(),
                marks(&[
                    ("hi", Mark::Bold),
                    ("you", Mark::Italic),
                    ("no", Mark::Strikethrough),
                    ("x", Mark::Code)
                ])
            )
        );
    }

    #[test]
    fn accepts_markdown_bold_and_nesting() {
        assert_eq!(
            styled("**Next step:** *_now_*"),
            (
                "Next step: now".to_owned(),
                marks(&[
                    ("Next step:", Mark::Bold),
                    ("now", Mark::Italic),
                    ("now", Mark::Bold)
                ])
            )
        );
    }

    #[test]
    fn leaves_symbols_inside_words_and_unclosed_ones_alone() {
        assert_eq!(
            styled("snake_case 2*3*4 *open"),
            ("snake_case 2*3*4 *open".to_owned(), marks(&[]))
        );
    }

    #[test]
    fn reads_code_blocks_quotes_and_lists() {
        assert_eq!(
            styled("```\nlet *x* = 1;\n```\n> said\n- item"),
            (
                "let *x* = 1;\nsaid\n• item".to_owned(),
                marks(&[("let *x* = 1;", Mark::CodeBlock), ("said", Mark::Quote)])
            )
        );
        assert_eq!(
            styled("```rust\nfn main() {}\n```"),
            (
                "fn main() {}".to_owned(),
                marks(&[("fn main() {}", Mark::CodeBlock)])
            )
        );
    }
}
