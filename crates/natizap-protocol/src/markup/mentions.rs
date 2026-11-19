//! Mentions: WhatsApp writes `@` and the person's number (or their LID's
//! digits) into the text, and each client shows the name it knows instead.

use std::ops::Range;

use super::{Mark, Marked};

/// The fewest and most digits a mention's number has.
const DIGITS: Range<usize> = 5..21;

/// A mention starting at `chars[start]`: where it ends. It stands alone,
/// so an email address's `@` is not one.
pub(super) fn mention(chars: &[(usize, char)], start: usize) -> Option<usize> {
    if chars[start].1 != '@' || start > 0 && chars[start - 1].1.is_alphanumeric() {
        return None;
    }
    let end = (start + 1..chars.len())
        .find(|&i| !chars[i].1.is_ascii_digit())
        .unwrap_or(chars.len());
    let alone = chars.get(end).is_none_or(|&(_, c)| !c.is_alphanumeric());
    (alone && DIGITS.contains(&(end - start - 1))).then_some(end)
}

impl Marked {
    /// Shows each mention as `@` and the name `name` gives for its digits;
    /// mentions it knows no name for keep their digits. Returns where each
    /// mention now stands, and the digits it was written with.
    pub fn name_mentions(
        &mut self,
        name: impl Fn(&str) -> Option<String>,
    ) -> Vec<(Range<usize>, String)> {
        let mentions: Vec<Range<usize>> = self.mentions().collect();
        let digits: Vec<String> = mentions
            .iter()
            .map(|range| self.text[range.start + 1..range.end].to_owned())
            .collect();
        // From the end, so the ranges still to rename stay where they are.
        for (range, digits) in mentions.into_iter().zip(&digits).rev() {
            let Some(shown) = name(digits) else {
                continue;
            };
            let shown = format!("@{shown}");
            let grown = shown.len().cast_signed() - range.len().cast_signed();
            self.text.replace_range(range.clone(), &shown);
            let shift = |at: usize| {
                if at >= range.end {
                    at.saturating_add_signed(grown)
                } else {
                    at
                }
            };
            for (marked, _) in &mut self.marks {
                *marked = shift(marked.start)..shift(marked.end);
            }
        }
        self.mentions().zip(digits).collect()
    }

    fn mentions(&self) -> impl Iterator<Item = Range<usize>> + use<'_> {
        self.marks
            .iter()
            .filter(|(_, mark)| *mark == Mark::Mention)
            .map(|(range, _)| range.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_mentions_and_shows_the_names_known() {
        let mut marked = Marked::parse("*@5511912345678 and @99887766* see a@b.com @12");
        let mentions =
            marked.name_mentions(|digits| (digits == "5511912345678").then(|| "Ana".to_owned()));

        assert_eq!(marked.text, "@Ana and @99887766 see a@b.com @12");
        assert_eq!(
            mentions,
            [
                (0..4, "5511912345678".to_owned()),
                (9..18, "99887766".to_owned())
            ]
        );
        let shown: Vec<(&str, Mark)> = marked
            .marks
            .iter()
            .map(|(range, mark)| (&marked.text[range.clone()], *mark))
            .filter(|(_, mark)| matches!(mark, Mark::Mention | Mark::Bold))
            .collect();
        assert!(shown.contains(&("@Ana", Mark::Mention)));
        assert!(shown.contains(&("@99887766", Mark::Mention)));
        assert!(shown.contains(&("@Ana and @99887766", Mark::Bold)));
        assert_eq!(shown.len(), 3);
    }
}
