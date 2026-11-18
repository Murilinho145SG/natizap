//! Matching a search in text: whether it is there, and where, telling
//! capitals apart only when asked.

use std::ops::Range;

/// Whether a search tells capitals from small letters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Case {
    Sensitive,
    Insensitive,
}

impl Case {
    /// Whether `text` holds `query` under this rule.
    pub(crate) fn finds(self, text: &str, query: &str) -> bool {
        !occurrences(text, query, self).is_empty()
    }
}

/// Where `query` is in `text`, as byte ranges of `text`, left to right
/// and not overlapping.
pub(crate) fn occurrences(text: &str, query: &str, case: Case) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    match case {
        Case::Sensitive => text
            .match_indices(query)
            .map(|(start, found)| start..start + found.len())
            .collect(),
        Case::Insensitive => {
            // Lowering can change a letter's length (İ, ẞ), so each byte
            // of the lowered text remembers the letter it came from.
            let mut lowered = String::with_capacity(text.len());
            let mut origin: Vec<Range<usize>> = Vec::with_capacity(text.len());
            for (start, ch) in text.char_indices() {
                let letter = start..start + ch.len_utf8();
                for low in ch.to_lowercase() {
                    lowered.push(low);
                    origin.extend(std::iter::repeat_n(letter.clone(), low.len_utf8()));
                }
            }
            lowered
                .match_indices(&query.to_lowercase())
                .filter_map(|(start, found)| {
                    let first = origin.get(start)?;
                    let last = origin.get(start + found.len() - 1)?;
                    Some(first.start..last.end)
                })
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_every_occurrence_where_it_is() {
        let text = "Não, NÃO e não";
        assert_eq!(
            occurrences(text, "não", Case::Insensitive)
                .into_iter()
                .map(|range| &text[range])
                .collect::<Vec<_>>(),
            ["Não", "NÃO", "não"]
        );
        assert_eq!(occurrences(text, "não", Case::Sensitive).len(), 1);
        assert_eq!(
            occurrences(text, "", Case::Insensitive),
            Vec::<Range<usize>>::new()
        );
    }

    #[test]
    fn maps_letters_whose_lowered_form_grows_back_to_the_text() {
        // İ lowers to two characters.
        let text = "aİb";
        let found = occurrences(text, "b", Case::Insensitive);
        assert_eq!(
            found.iter().map(|r| &text[r.clone()]).collect::<Vec<_>>(),
            ["b"]
        );
        assert!(Case::Insensitive.finds(text, "İ"));
    }
}
