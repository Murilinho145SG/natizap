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
    /// Every speed, slowest first, as the speed menu lists them.
    pub const ALL: [Self; 4] = [Self::Normal, Self::Faster, Self::Fast, Self::Double];

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
    fn offers_the_speeds_whatsapp_offers() {
        let factors: Vec<f32> = Speed::ALL.iter().map(|speed| speed.factor()).collect();
        assert_eq!(factors, [1.0, 1.25, 1.5, 2.0]);
    }
}
