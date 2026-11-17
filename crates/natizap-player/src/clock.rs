use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Where playback is. With sound, the sound card drives it: position is how
/// much audio it has played, so video follows the audio exactly. Without
/// sound, the wall clock does.
#[derive(Debug, Default)]
pub(crate) struct Clock {
    /// Audio frames (one sample per channel) played since `base`.
    played: AtomicU64,
    paused: AtomicBool,
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    /// Position when counting last restarted (start, seek, resume).
    base: Duration,
    /// Audio frames per second, when sound drives the clock.
    audio_rate: Option<u32>,
    /// When the wall clock last started counting, while playing silently.
    since: Option<Instant>,
}

impl Clock {
    /// Lets the sound card drive the clock at `rate` frames per second.
    pub(crate) fn drive_with_audio(&self, rate: u32) {
        self.lock().audio_rate = Some(rate);
    }

    /// The current position.
    pub(crate) fn position(&self) -> Duration {
        let state = self.lock();
        match state.audio_rate {
            Some(rate) => {
                let played = self.played.load(Ordering::Acquire);
                let whole = played / u64::from(rate);
                let rest = played % u64::from(rate);
                state.base
                    + Duration::from_secs(whole)
                    + Duration::from_nanos(rest * 1_000_000_000 / u64::from(rate))
            }
            None => state.base + state.since.map_or(Duration::ZERO, |since| since.elapsed()),
        }
    }

    /// Whether playback is paused.
    pub(crate) fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Acquire)
    }

    /// Records that the sound card played `frames` more audio frames.
    pub(crate) fn advance(&self, frames: u64) {
        self.played.fetch_add(frames, Ordering::AcqRel);
    }

    /// Pauses or resumes.
    pub(crate) fn set_paused(&self, paused: bool) {
        let position = self.position();
        let mut state = self.lock();
        self.paused.store(paused, Ordering::Release);
        if state.audio_rate.is_none() {
            state.base = position;
            state.since = (!paused).then(Instant::now);
        }
    }

    /// Restarts counting from `position`, e.g. after a seek.
    pub(crate) fn restart_at(&self, position: Duration) {
        let mut state = self.lock();
        self.played.store(0, Ordering::Release);
        state.base = position;
        state.since = (state.audio_rate.is_none() && !self.is_paused()).then(Instant::now);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        // A panic while holding the lock leaves the state consistent: every
        // write above is a single assignment.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_driven_position_counts_played_frames_from_the_seek_point() {
        let clock = Clock::default();
        clock.drive_with_audio(48_000);
        clock.restart_at(Duration::from_secs(10));
        clock.advance(24_000);
        assert_eq!(clock.position(), Duration::from_millis(10_500));
    }

    #[test]
    fn silent_playback_stops_counting_while_paused() {
        let clock = Clock::default();
        clock.restart_at(Duration::from_secs(3));
        clock.set_paused(true);
        let paused_at = clock.position();
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(clock.position(), paused_at);
    }
}
