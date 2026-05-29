//! The lock state machine: apply commands, confirm or fail transient moves,
//! and gate capability-restricted commands.
//!
//! Modelled on the Home Assistant `lock` entity domain (Apache-2.0): a command
//! is accepted only when it makes sense for the current state, an optimistic
//! lock reports a transient `Locking` / `Unlocking` state while the bolt moves,
//! and the lock then *confirms* (settles to `Locked` / `Unlocked` / `Open`) or
//! *fails* (settles to `Jammed`). The machine is the safety brain — it refuses
//! illegal transitions rather than guessing.

use crate::code::{CodeCredential, CodeVerdict, LockCode};
use crate::state::{LockCommand, LockState};

/// What a lock can do, beyond the universal lock/unlock. Vendors differ: a
/// simple deadbolt cannot retract its latch (`Open`); a smart latch can.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockFeatures {
    /// Whether the lock supports the [`LockCommand::Open`] command (retract the
    /// latch so the door swings free). Maps to HA's `LockEntityFeature.OPEN`.
    pub supports_open: bool,
}

impl LockFeatures {
    /// A plain deadbolt: lock and unlock only.
    #[must_use]
    pub const fn deadbolt() -> Self {
        Self { supports_open: false }
    }

    /// A latch-capable lock that can also pull the door open.
    #[must_use]
    pub const fn with_open() -> Self {
        Self { supports_open: true }
    }
}

/// Why a command was rejected by the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionError {
    /// The lock does not advertise the capability this command needs (e.g.
    /// `Open` on a plain deadbolt).
    Unsupported,
    /// The command is meaningless from the current state (e.g. `Lock` while a
    /// lock is already mid-`Locking`, or operating a `Jammed` lock that must be
    /// physically cleared first).
    IllegalTransition,
    /// A keypad command was issued but the presented PIN was wrong or the
    /// credential is locked out.
    CodeRejected,
}

impl core::fmt::Display for TransitionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unsupported => f.write_str("the lock does not support this action"),
            Self::IllegalTransition => f.write_str("the lock cannot do that right now"),
            Self::CodeRejected => f.write_str("the code was not accepted"),
        }
    }
}

impl std::error::Error for TransitionError {}

/// A door lock and its current settled (or transient) state.
#[derive(Debug, Clone)]
pub struct Lock {
    state: LockState,
    features: LockFeatures,
}

impl Lock {
    /// A newly seen lock whose real state is not yet known.
    #[must_use]
    pub const fn new(features: LockFeatures) -> Self {
        Self { state: LockState::Unknown, features }
    }

    /// A lock that has reported a known starting state.
    #[must_use]
    pub const fn with_state(features: LockFeatures, state: LockState) -> Self {
        Self { state, features }
    }

    #[must_use]
    pub const fn state(&self) -> LockState {
        self.state
    }

    #[must_use]
    pub const fn features(&self) -> LockFeatures {
        self.features
    }

    /// Whether `command` is legal from the current state, ignoring the optimism
    /// of the transition. This is the pure transition table.
    fn is_legal(&self, command: LockCommand) -> Result<(), TransitionError> {
        // Capability gate first: a missing capability is always Unsupported,
        // regardless of state, so the caller gets the most specific reason.
        if matches!(command, LockCommand::Open) && !self.features.supports_open {
            return Err(TransitionError::Unsupported);
        }
        // A jammed lock must be physically inspected and cleared; no software
        // command may move it. Surfacing the jam is the whole point.
        if matches!(self.state, LockState::Jammed) {
            return Err(TransitionError::IllegalTransition);
        }
        match (self.state, command) {
            // Already mid-flight in the same direction: redundant.
            (LockState::Locking, LockCommand::Lock)
            | (LockState::Unlocking, LockCommand::Unlock) => {
                Err(TransitionError::IllegalTransition)
            }
            // Everything else is a sensible move: locking a known-unlocked or
            // unknown lock, unlocking a locked one, re-issuing the opposite
            // command to abort an in-flight move, opening from any non-jammed
            // capable state, etc. The lock hardware is the final arbiter and
            // reports back via confirm/fail.
            _ => Ok(()),
        }
    }

    /// Apply a command optimistically. On success the lock enters the
    /// command's transient state (`Locking` / `Unlocking`) — or, for `Open`,
    /// goes straight to [`LockState::Open`], which has no in-flight phase.
    ///
    /// The hardware later calls [`Lock::confirm`] or [`Lock::fail`] to settle a
    /// transient move.
    ///
    /// # Errors
    /// [`TransitionError`] if the command is unsupported or illegal from the
    /// current state.
    pub fn apply(&mut self, command: LockCommand) -> Result<LockState, TransitionError> {
        self.is_legal(command)?;
        self.state = command
            .in_flight_state()
            .unwrap_or(LockState::Open);
        Ok(self.state)
    }

    /// Apply a keypad-gated command: the PIN is verified first, and the command
    /// is applied only on [`CodeVerdict::Accepted`].
    ///
    /// # Errors
    /// [`TransitionError::CodeRejected`] if the PIN is wrong or locked out;
    /// otherwise the same errors as [`Lock::apply`].
    pub fn apply_with_code(
        &mut self,
        command: LockCommand,
        credential: &mut CodeCredential,
        presented: &LockCode,
    ) -> Result<LockState, TransitionError> {
        // Check legality before spending an attempt, so an illegal command does
        // not burn the keypad lock-out budget.
        self.is_legal(command)?;
        match credential.verify(presented) {
            CodeVerdict::Accepted => self.apply(command),
            CodeVerdict::Rejected | CodeVerdict::LockedOut => {
                Err(TransitionError::CodeRejected)
            }
        }
    }

    /// The hardware confirms a transient move succeeded: `Locking` settles to
    /// `Locked`, `Unlocking` to `Unlocked`. Confirming from a settled state is
    /// a no-op (idempotent), so a late or duplicate confirmation is harmless.
    pub fn confirm(&mut self) {
        self.state = match self.state {
            LockState::Locking => LockState::Locked,
            LockState::Unlocking => LockState::Unlocked,
            other => other,
        };
    }

    /// The hardware reports the transient move physically failed (bolt blocked,
    /// motor stalled): the lock settles to [`LockState::Jammed`]. Failing from a
    /// settled state still surfaces the jam — a lock can report a jam any time
    /// it discovers one.
    pub fn fail(&mut self) {
        self.state = LockState::Jammed;
    }

    /// Clear a jam after a human has physically checked the door, telling the
    /// machine the now-known real state.
    ///
    /// # Errors
    /// [`TransitionError::IllegalTransition`] if the lock is not jammed (there
    /// is nothing to clear) or if `observed` is itself a non-settled state.
    pub fn clear_jam(&mut self, observed: LockState) -> Result<(), TransitionError> {
        if !matches!(self.state, LockState::Jammed) {
            return Err(TransitionError::IllegalTransition);
        }
        if observed.is_transient() || matches!(observed, LockState::Jammed) {
            return Err(TransitionError::IllegalTransition);
        }
        self.state = observed;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deadbolt(state: LockState) -> Lock {
        Lock::with_state(LockFeatures::deadbolt(), state)
    }

    #[test]
    fn lock_from_unlocked_goes_locking_then_locked() {
        let mut lock = deadbolt(LockState::Unlocked);
        assert_eq!(lock.apply(LockCommand::Lock), Ok(LockState::Locking));
        assert!(lock.state().is_transient());
        lock.confirm();
        assert_eq!(lock.state(), LockState::Locked);
        assert!(lock.state().is_secure());
    }

    #[test]
    fn unlock_from_locked_goes_unlocking_then_unlocked() {
        let mut lock = deadbolt(LockState::Locked);
        assert_eq!(lock.apply(LockCommand::Unlock), Ok(LockState::Unlocking));
        lock.confirm();
        assert_eq!(lock.state(), LockState::Unlocked);
    }

    #[test]
    fn locking_that_fails_becomes_jammed() {
        let mut lock = deadbolt(LockState::Unlocked);
        lock.apply(LockCommand::Lock).expect("legal");
        lock.fail();
        assert_eq!(lock.state(), LockState::Jammed);
        assert!(lock.state().needs_attention());
        assert!(!lock.state().is_secure());
    }

    #[test]
    fn commands_on_jammed_lock_are_rejected() {
        let mut lock = deadbolt(LockState::Jammed);
        assert_eq!(
            lock.apply(LockCommand::Lock),
            Err(TransitionError::IllegalTransition)
        );
        assert_eq!(
            lock.apply(LockCommand::Unlock),
            Err(TransitionError::IllegalTransition)
        );
        // The jam is preserved — a rejected command must not silently clear it.
        assert_eq!(lock.state(), LockState::Jammed);
    }

    #[test]
    fn open_unsupported_on_deadbolt() {
        let mut lock = deadbolt(LockState::Locked);
        assert_eq!(
            lock.apply(LockCommand::Open),
            Err(TransitionError::Unsupported)
        );
        assert_eq!(lock.state(), LockState::Locked, "rejected open changes nothing");
    }

    #[test]
    fn open_supported_settles_to_open() {
        let mut lock = Lock::with_state(LockFeatures::with_open(), LockState::Locked);
        assert_eq!(lock.apply(LockCommand::Open), Ok(LockState::Open));
        assert_eq!(lock.state(), LockState::Open);
    }

    #[test]
    fn redundant_in_flight_command_rejected() {
        let mut lock = deadbolt(LockState::Locking);
        assert_eq!(
            lock.apply(LockCommand::Lock),
            Err(TransitionError::IllegalTransition)
        );
    }

    #[test]
    fn opposite_command_can_abort_in_flight() {
        // Unlocking while a lock is mid-Locking is a legal direction reversal.
        let mut lock = deadbolt(LockState::Locking);
        assert_eq!(lock.apply(LockCommand::Unlock), Ok(LockState::Unlocking));
    }

    #[test]
    fn lock_from_unknown_is_allowed() {
        let mut lock = deadbolt(LockState::Unknown);
        assert_eq!(lock.apply(LockCommand::Lock), Ok(LockState::Locking));
    }

    #[test]
    fn confirm_is_idempotent_from_settled() {
        let mut lock = deadbolt(LockState::Locked);
        lock.confirm(); // nothing in flight
        assert_eq!(lock.state(), LockState::Locked);
    }

    #[test]
    fn unsupported_beats_jam_for_open() {
        // Even jammed, an Open on a non-open lock is reported as Unsupported —
        // the most specific, actionable reason.
        let mut lock = deadbolt(LockState::Jammed);
        assert_eq!(
            lock.apply(LockCommand::Open),
            Err(TransitionError::Unsupported)
        );
    }

    #[test]
    fn clear_jam_to_settled_state() {
        let mut lock = deadbolt(LockState::Jammed);
        assert!(lock.clear_jam(LockState::Locked).is_ok());
        assert_eq!(lock.state(), LockState::Locked);
    }

    #[test]
    fn clear_jam_rejects_non_jammed() {
        let mut lock = deadbolt(LockState::Locked);
        assert_eq!(
            lock.clear_jam(LockState::Unlocked),
            Err(TransitionError::IllegalTransition)
        );
    }

    #[test]
    fn clear_jam_rejects_transient_observation() {
        let mut lock = deadbolt(LockState::Jammed);
        assert_eq!(
            lock.clear_jam(LockState::Locking),
            Err(TransitionError::IllegalTransition)
        );
        assert_eq!(lock.state(), LockState::Jammed, "still jammed");
    }

    #[test]
    fn keypad_unlock_with_correct_code() {
        let stored = LockCode::parse("1379").expect("valid");
        let mut cred = CodeCredential::enroll(&stored);
        let mut lock = deadbolt(LockState::Locked);
        let attempt = LockCode::parse("1379").expect("valid");
        assert_eq!(
            lock.apply_with_code(LockCommand::Unlock, &mut cred, &attempt),
            Ok(LockState::Unlocking)
        );
    }

    #[test]
    fn keypad_unlock_with_wrong_code_rejected() {
        let stored = LockCode::parse("1379").expect("valid");
        let mut cred = CodeCredential::enroll(&stored);
        let mut lock = deadbolt(LockState::Locked);
        let attempt = LockCode::parse("0000").expect("valid");
        assert_eq!(
            lock.apply_with_code(LockCommand::Unlock, &mut cred, &attempt),
            Err(TransitionError::CodeRejected)
        );
        // A rejected code must not move the lock.
        assert_eq!(lock.state(), LockState::Locked);
    }

    #[test]
    fn illegal_command_does_not_spend_an_attempt() {
        let stored = LockCode::parse("1379").expect("valid");
        let mut cred = CodeCredential::enroll(&stored);
        let mut lock = deadbolt(LockState::Jammed);
        let attempt = LockCode::parse("1379").expect("valid");
        assert_eq!(
            lock.apply_with_code(LockCommand::Unlock, &mut cred, &attempt),
            Err(TransitionError::IllegalTransition)
        );
        assert_eq!(cred.failure_count(), 0, "illegal command must not burn the lock-out budget");
    }
}
