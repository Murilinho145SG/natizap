//! Where the caret stops when it moves by a character or by a word.
//!
//! Offsets are byte offsets into the text and always land on grapheme
//! boundaries, so an accented letter or an emoji moves as one.

use unicode_segmentation::UnicodeSegmentation as _;

/// What a grapheme is, for deciding where a word ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Space,
    Word,
    Punctuation,
}

fn class(grapheme: &str) -> Class {
    match grapheme.chars().next() {
        Some(ch) if ch.is_whitespace() => Class::Space,
        Some(ch) if ch.is_alphanumeric() || ch == '_' => Class::Word,
        _ => Class::Punctuation,
    }
}

/// The grapheme boundary before `offset`, or 0.
pub(crate) fn previous_grapheme(text: &str, offset: usize) -> usize {
    text.grapheme_indices(true)
        .rev()
        .find_map(|(index, _)| (index < offset).then_some(index))
        .unwrap_or(0)
}

/// The grapheme boundary after `offset`, or the end of `text`.
pub(crate) fn next_grapheme(text: &str, offset: usize) -> usize {
    text.grapheme_indices(true)
        .find_map(|(index, _)| (index > offset).then_some(index))
        .unwrap_or(text.len())
}

/// Where Ctrl+Left lands from `offset`: past any spaces, at the start of
/// the run of letters or of punctuation before them.
pub(crate) fn previous_word(text: &str, offset: usize) -> usize {
    let mut graphemes = text[..offset]
        .grapheme_indices(true)
        .rev()
        .skip_while(|(_, grapheme)| class(grapheme) == Class::Space)
        .peekable();
    let Some(&(mut start, first)) = graphemes.peek() else {
        return 0;
    };
    let run = class(first);
    for (index, grapheme) in graphemes {
        if class(grapheme) != run {
            break;
        }
        start = index;
    }
    start
}

/// Where Ctrl+Right lands from `offset`: past any spaces, at the end of the
/// run of letters or of punctuation after them.
pub(crate) fn next_word(text: &str, offset: usize) -> usize {
    let mut graphemes = text[offset..]
        .grapheme_indices(true)
        .map(|(index, grapheme)| (offset + index, grapheme))
        .skip_while(|(_, grapheme)| class(grapheme) == Class::Space)
        .peekable();
    let Some(&(_, first)) = graphemes.peek() else {
        return text.len();
    };
    let run = class(first);
    graphemes
        .find(|(_, grapheme)| class(grapheme) != run)
        .map_or(text.len(), |(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "é o 7490-1/04, então";

    #[test]
    fn previous_word_skips_spaces_then_one_word() {
        assert_eq!(
            previous_word(TEXT, TEXT.len()),
            TEXT.find("então").unwrap_or(0)
        );
        assert_eq!(previous_word("olá   ", 6), 0);
    }

    #[test]
    fn previous_word_stops_at_punctuation() {
        let after_04 = TEXT.find(',').unwrap_or(0);
        assert_eq!(previous_word(TEXT, after_04), TEXT.find("04").unwrap_or(0));
        let after_comma = after_04 + 1;
        assert_eq!(previous_word(TEXT, after_comma), after_04);
    }

    #[test]
    fn previous_word_treats_accented_letters_as_letters() {
        assert_eq!(previous_word("ação", "ação".len()), 0);
        assert_eq!(previous_word(TEXT, 2), 0);
    }

    #[test]
    fn next_word_skips_spaces_then_one_word() {
        assert_eq!(next_word(TEXT, 2), TEXT.find(" 7490").unwrap_or(0));
        assert_eq!(next_word(TEXT, TEXT.find("então").unwrap_or(0)), TEXT.len());
    }

    #[test]
    fn next_word_stops_at_punctuation() {
        let at_number = TEXT.find("7490").unwrap_or(0);
        assert_eq!(next_word(TEXT, at_number), TEXT.find('-').unwrap_or(0));
    }

    #[test]
    fn word_motion_stays_put_at_the_ends() {
        assert_eq!(previous_word(TEXT, 0), 0);
        assert_eq!(next_word(TEXT, TEXT.len()), TEXT.len());
        assert_eq!(next_word("   ", 0), 3);
    }

    #[test]
    fn grapheme_motion_moves_over_whole_emoji() {
        let text = "a👍🏽b";
        assert_eq!(next_grapheme(text, 1), text.len() - 1);
        assert_eq!(previous_grapheme(text, text.len() - 1), 1);
    }
}
