//! The two lines of your profile you can write: their limits, as the
//! phone sets them.

/// A line of your profile that can be edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Field {
    Name,
    About,
}

impl Field {
    /// What the row is called.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::About => "About",
        }
    }

    /// The most characters it may hold, as the phone allows.
    fn limit(self) -> usize {
        match self {
            Self::Name => 25,
            Self::About => 139,
        }
    }

    /// `text`, trimmed, if it may be saved as this field.
    ///
    /// # Errors
    ///
    /// Says why not: it is empty, or longer than the phone allows.
    pub(crate) fn check(self, text: &str) -> Result<String, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err(format!("{} can't be empty", self.label()));
        }
        let count = text.chars().count();
        if count > self.limit() {
            return Err(format!(
                "{} can be at most {} characters ({count} now)",
                self.label(),
                self.limit()
            ));
        }
        Ok(text.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::Field;

    #[test]
    fn trims_what_is_saved() {
        assert_eq!(Field::Name.check("  Dani  "), Ok("Dani".to_owned()));
    }

    #[test]
    fn refuses_an_empty_line() {
        assert!(Field::Name.check("   ").is_err());
        assert!(Field::About.check("").is_err());
    }

    #[test]
    fn counts_characters_not_bytes() {
        let name = "é".repeat(25);
        assert!(Field::Name.check(&name).is_ok());
        assert!(Field::Name.check(&format!("{name}é")).is_err());
        assert!(Field::About.check(&"a".repeat(139)).is_ok());
        assert!(Field::About.check(&"a".repeat(140)).is_err());
    }
}
