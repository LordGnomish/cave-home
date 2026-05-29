//! Cover motion state — the five-value lifecycle of a cover, mirroring the
//! Home Assistant `cover` entity domain semantics (Apache-2.0), implemented
//! from those public semantics rather than ported source.

use crate::position::Position;

/// The motion state of a cover.
///
/// `Opening` / `Closing` are transient (the motor is running); `Open` /
/// `Closed` / `Stopped` are at-rest. A cover that has stopped part-way is
/// `Stopped`, not `Open` — the distinction matters for both the UI and for
/// deciding whether a fresh `Stop` is a no-op.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverState {
    /// Fully open and at rest.
    Open,
    /// Fully closed and at rest.
    Closed,
    /// Motor running toward the open end.
    Opening,
    /// Motor running toward the closed end.
    Closing,
    /// At rest part-way (or halted by Stop / obstruction).
    Stopped,
}

impl CoverState {
    /// Derive the at-rest state implied by a position: fully open -> `Open`,
    /// fully closed -> `Closed`, anything between -> `Stopped`.
    ///
    /// This never returns `Opening`/`Closing` — those describe an in-flight
    /// motor, which a position alone cannot tell you about.
    #[must_use]
    pub const fn at_rest_for(position: Position) -> Self {
        if position.is_closed() {
            Self::Closed
        } else if position.is_open() {
            Self::Open
        } else {
            Self::Stopped
        }
    }

    /// Whether the motor is currently running.
    #[must_use]
    pub const fn is_moving(self) -> bool {
        matches!(self, Self::Opening | Self::Closing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_rest_for_endpoints_and_middle() {
        assert_eq!(CoverState::at_rest_for(Position::CLOSED), CoverState::Closed);
        assert_eq!(CoverState::at_rest_for(Position::OPEN), CoverState::Open);
        assert_eq!(
            CoverState::at_rest_for(Position::new(50).unwrap()),
            CoverState::Stopped
        );
        assert_eq!(
            CoverState::at_rest_for(Position::new(1).unwrap()),
            CoverState::Stopped
        );
        assert_eq!(
            CoverState::at_rest_for(Position::new(99).unwrap()),
            CoverState::Stopped
        );
    }

    #[test]
    fn is_moving_only_for_transient_states() {
        assert!(CoverState::Opening.is_moving());
        assert!(CoverState::Closing.is_moving());
        assert!(!CoverState::Open.is_moving());
        assert!(!CoverState::Closed.is_moving());
        assert!(!CoverState::Stopped.is_moving());
    }
}
