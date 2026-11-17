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

#[derive(Debug)]
struct State {
    /// Position when counting last restarted (start, seek, resume).
    base: Duration,
    /// Audio frames per second, when sound drives the clock.
    audio_rate: Option<u32>,
    /// When the wall clock last started counting, while playing silently.
    since: Option<Instant>,
    /// Media seconds per real second.
    speed: f64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            base: Duration::ZERO,
            audio_rate: None,
            since: None,
            speed: 1.0,
        }
    }
}

impl Clock {
    /// Lets the sound card drive the clock at `rate` frames per second.
    pub(crate) fn drive_with_audio(&self, rate: u32) {
        self.lock().audio_rate = Some(rate);
    }

    /// The current position.
    pub(crate) fn position(&self) -> Duration {
        let state = self.lock();
        let real = if let Some(rate) = state.audio_rate {
            let played = self.played.load(Ordering::Acquire);
            let rate = u64::from(rate);
            Duration::from_secs(played / rate)
                + Duration::from_nanos(played % rate * 1_000_000_000 / rate)
        } else {
            state.since.map_or(Duration::ZERO, |since| since.elapsed())
        };
        state.base + real.mul_f64(state.speed)
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

    /// Plays `speed` times as fast from here on.
    pub(crate) fn set_speed(&self, speed: f32) {
        let position = self.position();
        self.lock().speed = f64::from(speed);
        self.restart_at(position);
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
    fn faster_playback_covers_more_media_per_played_frame() {
        let clock = Clock::default();
        clock.drive_with_audio(48_000);
        clock.restart_at(Duration::from_secs(2));
        clock.set_speed(2.0);
        clock.advance(48_000);
        assert_eq!(clock.position(), Duration::from_secs(4));
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
