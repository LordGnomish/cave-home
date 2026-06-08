//! The lock state model and the commands a household can issue.
//!
//! These mirror the Home Assistant `lock` entity domain semantics (Apache-2.0):
//! a lock is in one of a small set of states, transient states (`Locking` /
//! `Unlocking`) resolve to a settled state on confirmation, `Jammed` is the
//! safety-critical failure surface, and `Open` is the latch-retracted state for
//! the locks that physically support it.
//!
//! Nothing here touches a vendor, a radio or a network — the wire adapters that
//! drive these transitions are Phase-1b (see `parity.manifest.toml`).

/// The state of a single door lock.
///
/// Ordered by the HA `lock` domain, not by "how locked" — equality and pattern
/// matching are the only comparisons that make sense for a lock state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LockState {
    /// The bolt is thrown — the door is secured.
    Locked,
    /// The bolt is retracted — the door can be opened by hand.
    Unlocked,
    /// A lock command is in flight; the bolt is being thrown.
    Locking,
    /// An unlock command is in flight; the bolt is being retracted.
    Unlocking,
    /// The lock tried to move and physically could not (bolt blocked, motor
    /// stalled). Safety-critical: the door's real security is **unknown** and a
    /// human must check it.
    Jammed,
    /// The latch itself is held open (the door can swing freely). Only locks
    /// that advertise the open capability ever reach this state.
    Open,
    /// The lock's state has not been observed yet (just paired, or it dropped
    /// off and has not reported back).
    Unknown,
}

impl LockState {
    /// Whether this is a transient ("optimistic") state that is expected to
    /// resolve to a settled state once the lock confirms.
    #[must_use]
    pub const fn is_transient(self) -> bool {
        matches!(self, Self::Locking | Self::Unlocking)
    }

    /// Whether the door is secured. Only [`LockState::Locked`] is secure — a
    /// jam or an unknown state is explicitly **not** treated as secure, because
    /// a safety-critical surface must fail closed in its reporting.
    #[must_use]
    pub const fn is_secure(self) -> bool {
        matches!(self, Self::Locked)
    }

    /// Whether this state needs a human to physically look at the door.
    #[must_use]
    pub const fn needs_attention(self) -> bool {
        matches!(self, Self::Jammed | Self::Unknown)
    }
}

/// A command a household (or an automation) can issue to a lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LockCommand {
    /// Throw the bolt.
    Lock,
    /// Retract the bolt.
    Unlock,
    /// Retract the latch so the door can be opened (capability-gated).
    Open,
}

impl LockCommand {
    /// The transient state this command drives the lock into while it is in
    /// flight, for an optimistic lock that reports progress.
    ///
    /// `Open` has no distinct transient state in the HA domain — it is applied
    /// and then the lock settles on [`LockState::Open`].
    #[must_use]
    pub const fn in_flight_state(self) -> Option<LockState> {
        match self {
            Self::Lock => Some(LockState::Locking),
            Self::Unlock => Some(LockState::Unlocking),
            Self::Open => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_locking_and_unlocking_are_transient() {
        assert!(LockState::Locking.is_transient());
        assert!(LockState::Unlocking.is_transient());
        assert!(!LockState::Locked.is_transient());
        assert!(!LockState::Unlocked.is_transient());
        assert!(!LockState::Jammed.is_transient());
        assert!(!LockState::Open.is_transient());
        assert!(!LockState::Unknown.is_transient());
    }

    #[test]
    fn only_locked_is_secure() {
        assert!(LockState::Locked.is_secure());
        for s in [
            LockState::Unlocked,
            LockState::Locking,
            LockState::Unlocking,
            LockState::Jammed,
            LockState::Open,
            LockState::Unknown,
        ] {
            assert!(!s.is_secure(), "{s:?} must not report as secure");
        }
    }

    #[test]
    fn jam_and_unknown_need_attention() {
        assert!(LockState::Jammed.needs_attention());
        assert!(LockState::Unknown.needs_attention());
        assert!(!LockState::Locked.needs_attention());
        assert!(!LockState::Unlocked.needs_attention());
    }

    #[test]
    fn command_in_flight_states() {
        assert_eq!(LockCommand::Lock.in_flight_state(), Some(LockState::Locking));
        assert_eq!(
            LockCommand::Unlock.in_flight_state(),
            Some(LockState::Unlocking)
        );
        assert_eq!(LockCommand::Open.in_flight_state(), None);
    }
}
