//! The pure lifecycle state machine for a config entry.
//!
//! HA drives every config entry through a small set of states. cave-home keeps
//! the same semantics but as **pure transitions**: no async, no timers, no I/O.
//! The async setup execution that *causes* these transitions (actually calling
//! an integration's connect routine) is Phase 1b; this module is the
//! decision-only core it will sit on top of.
//!
//! The key piece of intelligence here is classifying a setup failure as
//! **transient** (worth retrying on our own — [`State::SetupRetry`]) versus
//! **permanent** (the household must intervene — [`State::SetupError`]). Only
//! the transient kind keeps the friendly "we'll keep trying" message going.

/// Where a config entry is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Known but not started.
    NotLoaded,
    /// Connecting / initialising right now.
    SettingUp,
    /// Up and running.
    Loaded,
    /// Setup failed permanently — needs the household to fix something.
    SetupError,
    /// Setup failed transiently — the engine will keep retrying on its own.
    SetupRetry,
    /// Being upgraded to a newer config schema.
    Migrating,
    /// Being shut down.
    Unloading,
    /// Permanently failed after giving up (or unloadable).
    Failed,
}

impl State {
    /// Whether the entry is currently providing its capabilities.
    #[must_use]
    pub const fn is_running(self) -> bool {
        matches!(self, Self::Loaded)
    }

    /// Whether the engine should, on its own, attempt another setup.
    #[must_use]
    pub const fn wants_retry(self) -> bool {
        matches!(self, Self::SetupRetry)
    }

    /// Whether the household needs to do something to move forward.
    #[must_use]
    pub const fn needs_attention(self) -> bool {
        matches!(self, Self::SetupError | Self::Failed)
    }
}

/// Why a setup attempt failed, before classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// Could not reach the device/service (offline, DNS, timeout). Transient.
    Unreachable,
    /// The connection dropped mid-setup. Transient.
    ConnectionLost,
    /// The device/service is busy / rate-limiting us. Transient.
    Busy,
    /// Credentials were rejected. Permanent — needs the household.
    BadCredentials,
    /// The config is malformed / incompatible. Permanent.
    BadConfig,
    /// The device firmware / API is unsupported. Permanent.
    Unsupported,
}

impl Failure {
    /// Whether this failure is worth retrying without bothering the household.
    #[must_use]
    pub const fn is_transient(self) -> bool {
        matches!(self, Self::Unreachable | Self::ConnectionLost | Self::Busy)
    }

    /// The state a fresh setup attempt lands in given this failure.
    #[must_use]
    pub const fn outcome_state(self) -> State {
        if self.is_transient() {
            State::SetupRetry
        } else {
            State::SetupError
        }
    }
}

/// An event that drives the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    /// Begin a setup attempt.
    BeginSetup,
    /// Setup succeeded.
    SetupSucceeded,
    /// Setup failed with the given reason.
    SetupFailed(Failure),
    /// A retry timer elapsed — try again.
    Retry,
    /// Begin a config-schema migration.
    BeginMigration,
    /// Migration finished; ready to set up again.
    MigrationDone,
    /// Begin unloading.
    BeginUnload,
    /// Unload finished.
    Unloaded,
    /// Give up retrying.
    GiveUp,
}

/// Why a transition was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransitionError {
    pub from: State,
    pub event: Transition,
}

impl core::fmt::Display for TransitionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "no transition from {:?} on {:?}", self.from, self.event)
    }
}

impl std::error::Error for TransitionError {}

/// Apply a transition to a state, returning the next state or rejecting the
/// event as invalid for the current state.
///
/// This is the whole machine: every legal edge is enumerated; anything not
/// listed is a [`TransitionError`] rather than a silent no-op, so a caller that
/// drives the machine wrongly hears about it.
///
/// # Errors
/// Returns [`TransitionError`] if `event` is not legal in state `current`.
pub const fn next(current: State, event: Transition) -> Result<State, TransitionError> {
    let next = match (current, event) {
        // Start setting up from a resting / failed / retrying state.
        (State::NotLoaded | State::SetupRetry | State::SetupError, Transition::BeginSetup)
        | (State::SetupRetry, Transition::Retry)
        | (State::Migrating, Transition::MigrationDone) => Some(State::SettingUp),

        // Setup resolves.
        (State::SettingUp, Transition::SetupSucceeded) => Some(State::Loaded),
        (State::SettingUp, Transition::SetupFailed(f)) => Some(f.outcome_state()),

        // Migration.
        (State::NotLoaded | State::Loaded, Transition::BeginMigration) => Some(State::Migrating),

        // Unloading.
        (
            State::Loaded | State::SetupError | State::SetupRetry,
            Transition::BeginUnload,
        ) => Some(State::Unloading),
        (State::Unloading, Transition::Unloaded) => Some(State::NotLoaded),

        // Give up retrying.
        (State::SetupRetry, Transition::GiveUp) => Some(State::Failed),

        _ => None,
    };
    match next {
        Some(s) => Ok(s),
        None => Err(TransitionError { from: current, event }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_setup_to_loaded() {
        let s = next(State::NotLoaded, Transition::BeginSetup).expect("begin");
        assert_eq!(s, State::SettingUp);
        let s = next(s, Transition::SetupSucceeded).expect("succeed");
        assert_eq!(s, State::Loaded);
        assert!(s.is_running());
    }

    #[test]
    fn transient_failure_goes_to_retry() {
        let s = next(State::SettingUp, Transition::SetupFailed(Failure::Unreachable)).expect("fail");
        assert_eq!(s, State::SetupRetry);
        assert!(s.wants_retry());
        assert!(!s.needs_attention());
    }

    #[test]
    fn permanent_failure_goes_to_error() {
        let s = next(State::SettingUp, Transition::SetupFailed(Failure::BadCredentials))
            .expect("fail");
        assert_eq!(s, State::SetupError);
        assert!(!s.wants_retry());
        assert!(s.needs_attention());
    }

    #[test]
    fn failure_classification_split() {
        assert!(Failure::Unreachable.is_transient());
        assert!(Failure::ConnectionLost.is_transient());
        assert!(Failure::Busy.is_transient());
        assert!(!Failure::BadCredentials.is_transient());
        assert!(!Failure::BadConfig.is_transient());
        assert!(!Failure::Unsupported.is_transient());
    }

    #[test]
    fn retry_re_enters_setup() {
        let s = next(State::SetupRetry, Transition::Retry).expect("retry");
        assert_eq!(s, State::SettingUp);
    }

    #[test]
    fn reload_is_unload_then_setup() {
        let s = next(State::Loaded, Transition::BeginUnload).expect("unload");
        assert_eq!(s, State::Unloading);
        let s = next(s, Transition::Unloaded).expect("unloaded");
        assert_eq!(s, State::NotLoaded);
        let s = next(s, Transition::BeginSetup).expect("re-setup");
        assert_eq!(s, State::SettingUp);
    }

    #[test]
    fn migration_round_trip() {
        let s = next(State::Loaded, Transition::BeginMigration).expect("migrate");
        assert_eq!(s, State::Migrating);
        let s = next(s, Transition::MigrationDone).expect("done");
        assert_eq!(s, State::SettingUp);
    }

    #[test]
    fn giving_up_after_retries_is_terminal_until_manual() {
        let s = next(State::SetupRetry, Transition::GiveUp).expect("give up");
        assert_eq!(s, State::Failed);
        assert!(s.needs_attention());
        // A household-initiated manual setup can still revive it... but only
        // from a state we allow BeginSetup; Failed needs no auto-revival.
        assert!(next(State::Failed, Transition::Retry).is_err());
    }

    #[test]
    fn illegal_transitions_are_rejected_not_ignored() {
        // Can't succeed setup we never began.
        assert!(next(State::NotLoaded, Transition::SetupSucceeded).is_err());
        // Can't retry something that's running fine.
        assert!(next(State::Loaded, Transition::Retry).is_err());
        // Can't unload something that was never loaded.
        assert!(next(State::NotLoaded, Transition::BeginUnload).is_err());
        let e = next(State::Loaded, Transition::Retry).unwrap_err();
        assert_eq!(e.from, State::Loaded);
    }

    #[test]
    fn manual_retry_from_permanent_error_is_allowed() {
        // The household fixed the password and asks us to try again.
        let s = next(State::SetupError, Transition::BeginSetup).expect("manual retry");
        assert_eq!(s, State::SettingUp);
    }
}
