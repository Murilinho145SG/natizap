//! How well a call's connection carries it, told from what its media
//! counted: the share of the other end's sound that had to be made up,
//! because it came late or not at all, over the last few seconds.

use std::collections::VecDeque;

use natizap_protocol::MediaCounts;

/// Seconds of counts the call's quality is judged over: long enough not
/// to flicker at one late packet, short enough to follow a change.
const WINDOW: usize = 4;

/// How well the connection carries the call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    /// Nothing worth noticing is lost.
    Good,
    /// Some sound is lost: words may break up now and then.
    Fair,
    /// Much is lost: the call is hard to follow.
    Poor,
}

/// Judges a call's quality from its media counts, taken once a second.
#[derive(Debug, Default)]
pub(crate) struct QualityWatch {
    counts: VecDeque<MediaCounts>,
    judged: Option<Quality>,
}

impl QualityWatch {
    /// Takes this second's `counts`; the quality they tell, once a window
    /// carried any sound.
    pub(crate) fn hear(&mut self, counts: MediaCounts) -> Option<Quality> {
        self.counts.push_back(counts);
        if self.counts.len() > WINDOW + 1 {
            self.counts.pop_front();
        }
        if let (Some(first), Some(last)) = (self.counts.front(), self.counts.back()) {
            let decoded = last.decoded.saturating_sub(first.decoded);
            let concealed = last.concealed.saturating_sub(first.concealed);
            let total = decoded + concealed;
            // A window with no sound at all says nothing new: the last
            // judgement stands.
            if total > 0 {
                let lost = f64::from(concealed) / f64::from(total);
                self.judged = Some(if lost < 0.05 {
                    Quality::Good
                } else if lost < 0.2 {
                    Quality::Fair
                } else {
                    Quality::Poor
                });
            }
        }
        self.judged
    }
}

#[cfg(test)]
mod tests {
    use natizap_protocol::MediaCounts;

    use super::{Quality, QualityWatch};

    /// Counts after `seconds`, losing `lost` of each 50 frames a second.
    fn after(seconds: u32, lost: u32) -> MediaCounts {
        MediaCounts {
            decoded: seconds * (50 - lost),
            concealed: seconds * lost,
        }
    }

    #[test]
    fn a_clean_line_is_good_and_a_lossy_one_poor() {
        let mut watch = QualityWatch::default();
        assert_eq!(watch.hear(after(0, 0)), None);
        assert_eq!(watch.hear(after(1, 0)), Some(Quality::Good));
        let mut lossy = QualityWatch::default();
        for second in 0..5 {
            lossy.hear(after(second, 20));
        }
        assert_eq!(lossy.hear(after(5, 20)), Some(Quality::Poor));
    }

    #[test]
    fn the_judgement_follows_the_last_few_seconds() {
        let mut watch = QualityWatch::default();
        let mut counts = MediaCounts::default();
        for _ in 0..6 {
            counts.decoded += 30;
            counts.concealed += 20;
            watch.hear(counts);
        }
        // The line clears; the old losses leave the window.
        let mut judged = None;
        for _ in 0..5 {
            counts.decoded += 50;
            judged = watch.hear(counts);
        }
        assert_eq!(judged, Some(Quality::Good));
    }

    #[test]
    fn silence_keeps_the_last_judgement() {
        let mut watch = QualityWatch::default();
        watch.hear(after(0, 0));
        watch.hear(after(1, 5));
        let still = after(1, 5);
        for _ in 0..6 {
            assert_eq!(watch.hear(still), Some(Quality::Fair));
        }
    }
}
