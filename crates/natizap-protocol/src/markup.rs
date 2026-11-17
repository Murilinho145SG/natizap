//! WhatsApp's text markup: `*bold*`, `_italic_`, `~strikethrough~`,
//! `` `code` ``, ```` ```code blocks``` ````, `> quotes` and `- lists`.
//! `**bold**` is accepted too, since bots often send Markdown.

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
    /// `` `code` `` or a ```` ``` ```` block.
    Code,
    /// A line starting with `> `.
    Quote,
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
                marked.text.push_str(block.trim_matches('\n'));
                marked.marks.push((start..marked.text.len(), Mark::Code));
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
                marks(&[("let *x* = 1;", Mark::Code), ("said", Mark::Quote)])
            )
        );
    }
}
