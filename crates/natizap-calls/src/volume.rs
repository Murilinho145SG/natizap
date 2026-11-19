//! How the other end of a call is heard: how loud the user set them, from
//! silent to twice as loud as they sent, kept from one call to the next;
//! whether the user muted them; and whether they turned their own
//! microphone off.

/// Whether the user hears the other end of a call.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Listening {
    /// As loud as the user set them.
    #[default]
    Heard,
    /// Muted for the user alone; the other end is not told.
    Muted,
}

impl Listening {
    /// The other way.
    #[must_use]
    pub fn toggled(self) -> Self {
        match self {
            Self::Heard => Self::Muted,
            Self::Muted => Self::Heard,
        }
    }
}

/// Whether the other end of a call has their microphone on, as they last
/// told: apart from the user muting them, which only the user hears.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PeerMicrophone {
    /// On.
    #[default]
    On,
    /// They turned it off.
    Off,
}

/// A person's loudness in calls, in percent of how they sent: 0 to 200.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Loudness(u16);

impl Default for Loudness {
    fn default() -> Self {
        Self::SENT
    }
}

impl Loudness {
    /// As loud as they sent.
    pub const SENT: Self = Self(100);
    /// The loudest: twice as loud as they sent.
    const MOST: u16 = 200;
    /// How near as sent a drag lands on it: as sent is worth finding again.
    const SNAP: u16 = 4;

    /// `percent` of how loud they sent, at most twice.
    #[must_use]
    pub fn new(percent: u16) -> Self {
        Self(percent.min(Self::MOST))
    }

    /// The loudness at `fraction` along a slider from silent to twice as
    /// loud, landing on as sent when near it.
    #[must_use]
    pub fn at(fraction: f32) -> Self {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to 0..=200 first"
        )]
        let percent = (fraction.clamp(0.0, 1.0) * f32::from(Self::MOST)).round() as u16;
        if percent.abs_diff(Self::SENT.0) <= Self::SNAP {
            Self::SENT
        } else {
            Self::new(percent)
        }
    }

    /// Where it stands along that slider.
    #[must_use]
    pub fn fraction(self) -> f32 {
        f32::from(self.0) / f32::from(Self::MOST)
    }

    /// The percent, as the user reads it.
    #[must_use]
    pub fn percent(self) -> u16 {
        self.0
    }

    /// What the speaker multiplies their sound by.
    pub(crate) fn gain(self) -> f32 {
        f32::from(self.0) / 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::Loudness;

    #[test]
    fn a_slider_runs_from_silent_to_twice_as_loud() {
        assert_eq!(Loudness::at(0.0).percent(), 0);
        assert_eq!(Loudness::at(1.0).percent(), 200);
        assert_eq!(Loudness::at(0.75).percent(), 150);
        assert!((Loudness::new(150).gain() - 1.5).abs() < f32::EPSILON);
    }

    #[test]
    fn near_as_sent_lands_on_it() {
        assert_eq!(Loudness::at(0.51), Loudness::SENT);
        assert_eq!(Loudness::at(0.48), Loudness::SENT);
        assert_eq!(Loudness::at(0.56).percent(), 112);
    }

    #[test]
    fn never_past_twice_as_loud() {
        assert_eq!(Loudness::new(900).percent(), 200);
    }
}
