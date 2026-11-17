/// How fast to play, as WhatsApp offers it for voice notes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Speed {
    /// Real time.
    #[default]
    Normal,
    /// 1.25×.
    Faster,
    /// 1.5×.
    Fast,
    /// 2×.
    Double,
}

impl Speed {
    /// Media seconds per real second.
    #[must_use]
    pub fn factor(self) -> f32 {
        match self {
            Self::Normal => 1.0,
            Self::Faster => 1.25,
            Self::Fast => 1.5,
            Self::Double => 2.0,
        }
    }

    /// The speed a tap on the speed button switches to; wraps to normal.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Normal => Self::Faster,
            Self::Faster => Self::Fast,
            Self::Fast => Self::Double,
            Self::Double => Self::Normal,
        }
    }

    /// What the speed button shows.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "1×",
            Self::Faster => "1.25×",
            Self::Fast => "1.5×",
            Self::Double => "2×",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tapping_cycles_through_every_speed_and_back() {
        let mut speed = Speed::Normal;
        let mut seen = Vec::new();
        for _ in 0..4 {
            seen.push(speed.factor());
            speed = speed.next();
        }
        assert_eq!(seen, [1.0, 1.25, 1.5, 2.0]);
        assert_eq!(speed, Speed::Normal);
    }
}
