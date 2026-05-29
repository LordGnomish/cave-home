//! The vacuum state model and the commands a household can issue.
//!
//! These mirror the Home Assistant `vacuum` entity domain and Valetudo's
//! control surface semantics (both permissive — see ADR-017): a vacuum is in
//! one of a small set of states, `Cleaning` / `SpotCleaning` / `Returning` are
//! the "busy" states, `Docked` is the resting/charging state, `Error` is the
//! safety surface that gates further commands until cleared, and `Manual` is
//! the human-driving (remote-control) state.
//!
//! Nothing here touches a vendor, a radio or a network — the wire adapters that
//! drive these transitions are Phase-1b (see `parity.manifest.toml`).

use crate::fan::FanSpeed;
use crate::map::{Segment, Zone};

/// The state of a single robot vacuum.
///
/// Ordered by the HA `vacuum` domain meaning, not by "how busy" — equality and
/// pattern matching are the only comparisons that make sense for a vacuum
/// state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VacuumState {
    /// Powered on, on the floor, but not cleaning and not on the dock.
    Idle,
    /// Actively cleaning the whole accessible area (or a chosen set of rooms).
    Cleaning,
    /// Cleaning a single small spot around its current position.
    SpotCleaning,
    /// Driving back to its charging dock.
    Returning,
    /// Parked on the dock (charging or fully charged).
    Docked,
    /// A clean was paused and can be resumed where it left off.
    Paused,
    /// The vacuum hit a problem it cannot solve on its own and needs a human.
    /// Safety surface: while in `Error`, software commands are gated until the
    /// fault is cleared.
    Error,
    /// A human is driving the vacuum by hand (remote control). No autonomous
    /// cleaning logic runs in this state.
    Manual,
}

impl VacuumState {
    /// Whether the vacuum is actively doing cleaning work right now.
    #[must_use]
    pub const fn is_cleaning(self) -> bool {
        matches!(self, Self::Cleaning | Self::SpotCleaning)
    }

    /// Whether the vacuum is busy moving for any reason (cleaning or going
    /// home). A busy vacuum is the set of states a low-battery check or a
    /// "send it home" command care about.
    #[must_use]
    pub const fn is_busy(self) -> bool {
        matches!(self, Self::Cleaning | Self::SpotCleaning | Self::Returning)
    }

    /// Whether this state needs a human to physically look at the vacuum.
    #[must_use]
    pub const fn needs_attention(self) -> bool {
        matches!(self, Self::Error)
    }

    /// Whether the vacuum is resting on its dock.
    #[must_use]
    pub const fn is_docked(self) -> bool {
        matches!(self, Self::Docked)
    }
}

/// A command a household (or an automation) can issue to a vacuum.
///
/// Mirrors the HA `vacuum` service set and Valetudo's command surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VacuumCommand {
    /// Begin (or resume) cleaning the whole accessible area.
    Start,
    /// Pause the current clean; it can be resumed where it left off.
    Pause,
    /// Stop the current clean and hold position (do not return to the dock).
    Stop,
    /// Drive back to the charging dock.
    ReturnToBase,
    /// Clean a single small spot around the current position.
    CleanSpot,
    /// Make a sound / flash so a person can find the vacuum.
    Locate,
    /// Set the suction power for subsequent cleaning.
    SetFanSpeed(FanSpeed),
    /// Clean a specific set of mapped rooms (by segment id).
    CleanSegments(Vec<u16>),
    /// Clean one or more rectangular zones.
    CleanZones(Vec<Zone>),
}

impl VacuumCommand {
    /// Whether issuing this command is meaningful while the vacuum is in an
    /// error state. Only [`VacuumCommand::Locate`] (help find it) and
    /// [`VacuumCommand::ReturnToBase`] (try to send it home) make sense while a
    /// fault is unresolved; everything else is gated until the error clears.
    ///
    /// This is advisory metadata; the authoritative gate lives in the machine.
    #[must_use]
    pub const fn allowed_in_error(&self) -> bool {
        matches!(self, Self::Locate | Self::ReturnToBase)
    }
}

/// A mapped room the household can name in a clean request, paired with the
/// segments behind it. Re-exported for convenience from this module's siblings.
pub type RoomRequest = Vec<Segment>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleaning_states_are_cleaning() {
        assert!(VacuumState::Cleaning.is_cleaning());
        assert!(VacuumState::SpotCleaning.is_cleaning());
        assert!(!VacuumState::Returning.is_cleaning());
        assert!(!VacuumState::Docked.is_cleaning());
    }

    #[test]
    fn busy_states_cover_cleaning_and_returning() {
        assert!(VacuumState::Cleaning.is_busy());
        assert!(VacuumState::SpotCleaning.is_busy());
        assert!(VacuumState::Returning.is_busy());
        assert!(!VacuumState::Idle.is_busy());
        assert!(!VacuumState::Paused.is_busy());
        assert!(!VacuumState::Docked.is_busy());
    }

    #[test]
    fn only_error_needs_attention() {
        assert!(VacuumState::Error.needs_attention());
        for s in [
            VacuumState::Idle,
            VacuumState::Cleaning,
            VacuumState::SpotCleaning,
            VacuumState::Returning,
            VacuumState::Docked,
            VacuumState::Paused,
            VacuumState::Manual,
        ] {
            assert!(!s.needs_attention(), "{s:?} must not need attention");
        }
    }

    #[test]
    fn only_locate_and_return_are_allowed_in_error() {
        assert!(VacuumCommand::Locate.allowed_in_error());
        assert!(VacuumCommand::ReturnToBase.allowed_in_error());
        assert!(!VacuumCommand::Start.allowed_in_error());
        assert!(!VacuumCommand::CleanSpot.allowed_in_error());
        assert!(!VacuumCommand::Pause.allowed_in_error());
    }
}
