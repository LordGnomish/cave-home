//! The playback-state model: what is playing, how loud, where in the song.
//!
//! [`Playback`] holds the live transport state (play / pause / stop), the
//! volume (0..=100), the current queue position and elapsed time, and the four
//! toggle [`Modes`](crate::tracklist::Modes). It is deliberately separate from
//! the [`Tracklist`](crate::tracklist::Tracklist): the queue owns *which* songs
//! exist, this type owns *what the player is doing* with them.

use crate::tracklist::Modes;

/// The transport state — MPD's `state` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayState {
    Play,
    Pause,
    #[default]
    Stop,
}

impl PlayState {
    /// The MPD wire token for this state.
    #[must_use]
    pub const fn as_mpd(self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Pause => "pause",
            Self::Stop => "stop",
        }
    }
}

/// Why a playback mutation was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackError {
    /// A volume outside 0..=100 was requested.
    VolumeOutOfRange,
    /// An elapsed/seek time that is negative or not finite.
    BadTime,
}

impl core::fmt::Display for PlaybackError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::VolumeOutOfRange => f.write_str("volume must be between 0 and 100"),
            Self::BadTime => f.write_str("time must be zero or more"),
        }
    }
}

impl std::error::Error for PlaybackError {}

/// The live transport state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Playback {
    state: PlayState,
    volume: u8,
    /// Current queue position, when something is loaded.
    current: Option<usize>,
    /// Seconds elapsed into the current song.
    elapsed: f64,
    modes: Modes,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            state: PlayState::Stop,
            volume: 50,
            current: None,
            elapsed: 0.0,
            modes: Modes::default(),
        }
    }
}

impl Playback {
    /// A stopped player at 50% volume with no song loaded.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub const fn state(&self) -> PlayState {
        self.state
    }

    #[must_use]
    pub const fn volume(&self) -> u8 {
        self.volume
    }

    #[must_use]
    pub const fn current(&self) -> Option<usize> {
        self.current
    }

    #[must_use]
    pub const fn elapsed(&self) -> f64 {
        self.elapsed
    }

    #[must_use]
    pub const fn modes(&self) -> Modes {
        self.modes
    }

    /// Set the volume.
    ///
    /// # Errors
    /// [`PlaybackError::VolumeOutOfRange`] if `level` is not within 0..=100.
    pub fn set_volume(&mut self, level: i64) -> Result<(), PlaybackError> {
        if (0..=100).contains(&level) {
            // Range-checked above; the cast cannot lose information.
            self.volume = u8::try_from(level).unwrap_or(0);
            Ok(())
        } else {
            Err(PlaybackError::VolumeOutOfRange)
        }
    }

    /// Begin (or resume) playback at a queue position.
    pub fn play_at(&mut self, pos: usize) {
        self.current = Some(pos);
        self.elapsed = 0.0;
        self.state = PlayState::Play;
    }

    /// Resume from pause without moving the cursor; if nothing is loaded this is
    /// a no-op that leaves the player stopped.
    pub fn resume(&mut self) {
        if self.current.is_some() {
            self.state = PlayState::Play;
        }
    }

    /// Pause if currently playing; otherwise leave the state alone.
    pub fn pause(&mut self) {
        if self.state == PlayState::Play {
            self.state = PlayState::Pause;
        }
    }

    /// Set the paused-state explicitly (MPD's `pause 0|1`).
    pub fn set_paused(&mut self, paused: bool) {
        match (paused, self.state) {
            (true, PlayState::Play) => self.state = PlayState::Pause,
            (false, PlayState::Pause) => self.state = PlayState::Play,
            _ => {}
        }
    }

    /// Stop: keep the cursor but reset elapsed and mark stopped.
    pub fn stop(&mut self) {
        self.state = PlayState::Stop;
        self.elapsed = 0.0;
    }

    /// Stop and unload the current song (the queue is now empty / cleared).
    pub fn unload(&mut self) {
        self.state = PlayState::Stop;
        self.current = None;
        self.elapsed = 0.0;
    }

    /// Set the elapsed position within the current song.
    ///
    /// # Errors
    /// [`PlaybackError::BadTime`] if `seconds` is negative or not finite.
    pub fn set_elapsed(&mut self, seconds: f64) -> Result<(), PlaybackError> {
        if seconds.is_finite() && seconds >= 0.0 {
            self.elapsed = seconds;
            Ok(())
        } else {
            Err(PlaybackError::BadTime)
        }
    }

    pub fn set_random(&mut self, on: bool) {
        self.modes.random = on;
    }

    pub fn set_repeat(&mut self, on: bool) {
        self.modes.repeat = on;
    }

    pub fn set_single(&mut self, on: bool) {
        self.modes.single = on;
    }

    pub fn set_consume(&mut self, on: bool) {
        self.modes.consume = on;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_stopped_half_volume_nothing_loaded() {
        let p = Playback::new();
        assert_eq!(p.state(), PlayState::Stop);
        assert_eq!(p.volume(), 50);
        assert_eq!(p.current(), None);
    }

    #[test]
    fn volume_accepts_bounds_and_rejects_outside() {
        let mut p = Playback::new();
        assert!(p.set_volume(0).is_ok());
        assert_eq!(p.volume(), 0);
        assert!(p.set_volume(100).is_ok());
        assert_eq!(p.volume(), 100);
        assert_eq!(p.set_volume(101), Err(PlaybackError::VolumeOutOfRange));
        assert_eq!(p.set_volume(-1), Err(PlaybackError::VolumeOutOfRange));
        // A rejected set leaves the prior value intact.
        assert_eq!(p.volume(), 100);
    }

    #[test]
    fn play_at_sets_cursor_and_resets_elapsed() {
        let mut p = Playback::new();
        p.set_elapsed(30.0).unwrap();
        p.play_at(2);
        assert_eq!(p.state(), PlayState::Play);
        assert_eq!(p.current(), Some(2));
        assert_eq!(p.elapsed(), 0.0);
    }

    #[test]
    fn pause_and_resume_round_trip() {
        let mut p = Playback::new();
        p.play_at(0);
        p.pause();
        assert_eq!(p.state(), PlayState::Pause);
        p.resume();
        assert_eq!(p.state(), PlayState::Play);
    }

    #[test]
    fn set_paused_explicit_only_toggles_between_play_and_pause() {
        let mut p = Playback::new();
        // Nothing loaded: stays stopped.
        p.set_paused(true);
        assert_eq!(p.state(), PlayState::Stop);
        p.play_at(0);
        p.set_paused(true);
        assert_eq!(p.state(), PlayState::Pause);
        p.set_paused(false);
        assert_eq!(p.state(), PlayState::Play);
    }

    #[test]
    fn resume_with_nothing_loaded_is_a_noop() {
        let mut p = Playback::new();
        p.resume();
        assert_eq!(p.state(), PlayState::Stop);
    }

    #[test]
    fn stop_keeps_cursor_unload_clears_it() {
        let mut p = Playback::new();
        p.play_at(1);
        p.stop();
        assert_eq!(p.state(), PlayState::Stop);
        assert_eq!(p.current(), Some(1));
        p.unload();
        assert_eq!(p.current(), None);
    }

    #[test]
    fn elapsed_rejects_negative_and_nonfinite() {
        let mut p = Playback::new();
        assert_eq!(p.set_elapsed(-1.0), Err(PlaybackError::BadTime));
        assert_eq!(p.set_elapsed(f64::NAN), Err(PlaybackError::BadTime));
        assert!(p.set_elapsed(12.5).is_ok());
        assert_eq!(p.elapsed(), 12.5);
    }

    #[test]
    fn mode_setters_flip_the_right_flag() {
        let mut p = Playback::new();
        p.set_random(true);
        p.set_consume(true);
        assert!(p.modes().random);
        assert!(p.modes().consume);
        assert!(!p.modes().repeat);
        assert!(!p.modes().single);
    }
}
