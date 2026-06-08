//! The media-player playback model.
//!
//! Mirrors the Home Assistant `media_player` domain's playback semantics
//! (Apache-2.0): a player that is showing media is in one of a small set of
//! states, and the transport verbs (play / pause / stop) move between them in a
//! constrained way. This module owns only the *playback* transition rules; the
//! power gating that wraps them lives in [`crate::machine`].

/// What the media player is currently doing with its content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    /// Content is playing.
    Playing,
    /// Content is loaded but paused.
    Paused,
    /// Playback is stopped — no active position.
    Stopped,
    /// On, but nothing is loaded (e.g. sitting on a home screen).
    Idle,
    /// Loading / buffering before playback resumes.
    Buffering,
}

impl PlaybackState {
    /// Apply a play request.
    ///
    /// Play resumes from [`Self::Paused`], starts from [`Self::Stopped`] or
    /// [`Self::Idle`], and settles a [`Self::Buffering`] player into playing.
    /// Playing while already playing is a no-op (stays playing).
    #[must_use]
    pub const fn play(self) -> Self {
        match self {
            Self::Playing | Self::Paused | Self::Stopped | Self::Idle | Self::Buffering => {
                Self::Playing
            }
        }
    }

    /// Apply a pause request.
    ///
    /// Only a playing or buffering player can be paused; pausing anything else
    /// returns the state unchanged so the caller can treat it as a no-op.
    #[must_use]
    pub const fn pause(self) -> Self {
        match self {
            Self::Playing | Self::Buffering => Self::Paused,
            other => other,
        }
    }

    /// Apply a stop request — always lands in [`Self::Stopped`].
    #[must_use]
    pub const fn stop(self) -> Self {
        Self::Stopped
    }

    /// Whether a transport (play/pause/seek/next/previous) makes sense here.
    ///
    /// A player sitting [`Self::Idle`] on a home screen has nothing to control;
    /// next/previous/seek need loaded content.
    #[must_use]
    pub const fn has_content(self) -> bool {
        !matches!(self, Self::Idle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_resumes_from_paused_and_stopped() {
        assert_eq!(PlaybackState::Paused.play(), PlaybackState::Playing);
        assert_eq!(PlaybackState::Stopped.play(), PlaybackState::Playing);
        assert_eq!(PlaybackState::Idle.play(), PlaybackState::Playing);
        assert_eq!(PlaybackState::Buffering.play(), PlaybackState::Playing);
    }

    #[test]
    fn pause_only_affects_playing_or_buffering() {
        assert_eq!(PlaybackState::Playing.pause(), PlaybackState::Paused);
        assert_eq!(PlaybackState::Buffering.pause(), PlaybackState::Paused);
        // No-op for the rest.
        assert_eq!(PlaybackState::Stopped.pause(), PlaybackState::Stopped);
        assert_eq!(PlaybackState::Idle.pause(), PlaybackState::Idle);
        assert_eq!(PlaybackState::Paused.pause(), PlaybackState::Paused);
    }

    #[test]
    fn stop_always_stops() {
        assert_eq!(PlaybackState::Playing.stop(), PlaybackState::Stopped);
        assert_eq!(PlaybackState::Idle.stop(), PlaybackState::Stopped);
    }

    #[test]
    fn idle_has_no_content() {
        assert!(!PlaybackState::Idle.has_content());
        assert!(PlaybackState::Playing.has_content());
        assert!(PlaybackState::Paused.has_content());
    }
}
