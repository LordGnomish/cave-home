//! The bootstrap state machine.
//!
//! A cave-home node walks a fixed sequence of milestones as the in-process K3s
//! components come up, mirroring the documented K3s server/agent boot:
//!
//! ```text
//! Initializing -> DatastoreReady -> ApiserverReady -> ControlPlaneReady
//!              -> NodeReady -> Running
//! ```
//!
//! Each step is gated on the previous one. A step can also *fail*; whether the
//! failure is retryable is classified, so a supervisor (phase-1b) knows whether
//! to back off and retry the same step or surface a fatal error. The machine
//! itself never blocks and never panics — it is a pure transition function the
//! caller drives.

use core::fmt;

/// Where a node is in its boot sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    /// Nothing is up yet; configuration validated, components selected.
    Initializing,
    /// The datastore (kine) is answering. (Server nodes only reach the later
    /// control-plane phases; agents skip straight from here conceptually, but
    /// the model keeps one linear ladder for both — an agent's "datastore" is
    /// the remote server's, already ready by the time it joins.)
    DatastoreReady,
    /// The apiserver is serving requests.
    ApiserverReady,
    /// Scheduler + controller-manager are up: the control plane is complete.
    ControlPlaneReady,
    /// The kubelet has registered and the node is Ready for workloads.
    NodeReady,
    /// Steady state — everything coordinated is up.
    Running,
}

impl Phase {
    /// The phase that should follow this one on success, or `None` if this is
    /// the terminal phase ([`Phase::Running`]).
    #[must_use]
    pub const fn next(self) -> Option<Self> {
        match self {
            Self::Initializing => Some(Self::DatastoreReady),
            Self::DatastoreReady => Some(Self::ApiserverReady),
            Self::ApiserverReady => Some(Self::ControlPlaneReady),
            Self::ControlPlaneReady => Some(Self::NodeReady),
            Self::NodeReady => Some(Self::Running),
            Self::Running => None,
        }
    }

    /// Whether this is the terminal running state.
    #[must_use]
    pub const fn is_running(self) -> bool {
        matches!(self, Self::Running)
    }
}

/// Why a bootstrap step failed, and whether retrying the same step can help.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// Transient: a dependency was not ready yet, a connection was refused,
    /// a timeout elapsed. Retrying the same step (after backoff) may succeed.
    Transient,
    /// Fatal: misconfiguration or a logically-impossible request. Retrying the
    /// same step cannot help; the caller must fix inputs and restart.
    Fatal,
}

impl FailureKind {
    /// Whether a supervisor should retry the failed step.
    #[must_use]
    pub const fn is_retryable(self) -> bool {
        matches!(self, Self::Transient)
    }
}

/// The driver: current phase plus the retry budget for the *current* step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bootstrap {
    phase: Phase,
    /// How many transient retries the current step has consumed.
    attempts_used: u32,
    /// Maximum transient retries allowed per step before it is declared fatal.
    max_attempts: u32,
    /// Set once a fatal (or budget-exhausted) failure has occurred; the machine
    /// then refuses to advance.
    aborted: Option<String>,
}

/// The result of feeding an outcome into the machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transition {
    /// Advanced to a new phase.
    Advanced(Phase),
    /// Reached and stayed at the terminal [`Phase::Running`].
    Running,
    /// A transient failure; the step will be retried (attempt number included).
    Retrying { phase: Phase, attempt: u32 },
    /// A fatal failure or exhausted retry budget; the machine is aborted.
    Aborted(String),
}

/// The maximum transient retries per step a default [`Bootstrap`] allows.
pub const DEFAULT_MAX_ATTEMPTS: u32 = 5;

impl Default for Bootstrap {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_ATTEMPTS)
    }
}

impl Bootstrap {
    /// A fresh machine at [`Phase::Initializing`] with the given per-step
    /// transient-retry budget.
    #[must_use]
    pub const fn new(max_attempts: u32) -> Self {
        Self {
            phase: Phase::Initializing,
            attempts_used: 0,
            max_attempts,
            aborted: None,
        }
    }

    /// The current phase.
    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// Whether the machine has aborted (and the reason, if so).
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn aborted_reason(&self) -> Option<&str> {
        self.aborted.as_deref()
    }

    /// Whether the machine has reached steady state.
    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.phase.is_running() && self.aborted.is_none()
    }

    /// Record that the current step *succeeded*, advancing one phase.
    ///
    /// Resets the per-step retry budget for the next step. A no-op (returns
    /// [`Transition::Running`]) at the terminal phase, and refuses to advance
    /// once aborted.
    pub fn succeed(&mut self) -> Transition {
        if let Some(reason) = &self.aborted {
            return Transition::Aborted(reason.clone());
        }
        match self.phase.next() {
            Some(next) => {
                self.phase = next;
                self.attempts_used = 0;
                if next.is_running() {
                    Transition::Running
                } else {
                    Transition::Advanced(next)
                }
            }
            None => Transition::Running,
        }
    }

    /// Record that the current step *failed* with the given classification.
    ///
    /// - [`FailureKind::Fatal`] aborts immediately.
    /// - [`FailureKind::Transient`] consumes a retry; once the budget is
    ///   exhausted the failure is escalated to an abort. The phase does **not**
    ///   advance on failure.
    pub fn fail(&mut self, kind: FailureKind, reason: &str) -> Transition {
        if let Some(existing) = &self.aborted {
            return Transition::Aborted(existing.clone());
        }
        match kind {
            FailureKind::Fatal => {
                let msg = format!("fatal at {:?}: {reason}", self.phase);
                self.aborted = Some(msg.clone());
                Transition::Aborted(msg)
            }
            FailureKind::Transient => {
                self.attempts_used = self.attempts_used.saturating_add(1);
                if self.attempts_used >= self.max_attempts {
                    let msg = format!(
                        "retry budget exhausted at {:?} after {} attempts: {reason}",
                        self.phase, self.attempts_used
                    );
                    self.aborted = Some(msg.clone());
                    Transition::Aborted(msg)
                } else {
                    Transition::Retrying {
                        phase: self.phase,
                        attempt: self.attempts_used,
                    }
                }
            }
        }
    }
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Initializing => "initializing",
            Self::DatastoreReady => "datastore-ready",
            Self::ApiserverReady => "apiserver-ready",
            Self::ControlPlaneReady => "control-plane-ready",
            Self::NodeReady => "node-ready",
            Self::Running => "running",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_ladder_is_linear_and_terminates() {
        let mut p = Phase::Initializing;
        let mut count = 1;
        while let Some(n) = p.next() {
            assert!(n > p, "phases must strictly increase");
            p = n;
            count += 1;
        }
        assert_eq!(p, Phase::Running);
        assert_eq!(count, 6, "six phases on the ladder");
        assert!(Phase::Running.next().is_none());
    }

    #[test]
    fn happy_path_walks_initializing_to_running() {
        let mut b = Bootstrap::default();
        assert_eq!(b.phase(), Phase::Initializing);
        assert_eq!(b.succeed(), Transition::Advanced(Phase::DatastoreReady));
        assert_eq!(b.succeed(), Transition::Advanced(Phase::ApiserverReady));
        assert_eq!(b.succeed(), Transition::Advanced(Phase::ControlPlaneReady));
        assert_eq!(b.succeed(), Transition::Advanced(Phase::NodeReady));
        assert_eq!(b.succeed(), Transition::Running);
        assert!(b.is_running());
        // Succeeding past Running is an idempotent no-op.
        assert_eq!(b.succeed(), Transition::Running);
        assert_eq!(b.phase(), Phase::Running);
    }

    #[test]
    fn transient_failure_retries_without_advancing() {
        let mut b = Bootstrap::new(5);
        // succeed once so we are at DatastoreReady, then fail transiently.
        b.succeed();
        assert_eq!(b.phase(), Phase::DatastoreReady);
        assert_eq!(
            b.fail(FailureKind::Transient, "conn refused"),
            Transition::Retrying {
                phase: Phase::DatastoreReady,
                attempt: 1
            }
        );
        // Phase did not move on failure.
        assert_eq!(b.phase(), Phase::DatastoreReady);
        // A subsequent success still advances and resets the retry budget.
        assert_eq!(b.succeed(), Transition::Advanced(Phase::ApiserverReady));
    }

    #[test]
    fn transient_failures_escalate_to_abort_when_budget_exhausted() {
        let mut b = Bootstrap::new(3);
        assert_eq!(
            b.fail(FailureKind::Transient, "x"),
            Transition::Retrying {
                phase: Phase::Initializing,
                attempt: 1
            }
        );
        assert_eq!(
            b.fail(FailureKind::Transient, "x"),
            Transition::Retrying {
                phase: Phase::Initializing,
                attempt: 2
            }
        );
        // Third attempt hits the budget (3) -> abort.
        match b.fail(FailureKind::Transient, "x") {
            Transition::Aborted(msg) => assert!(msg.contains("retry budget exhausted")),
            other => panic!("expected abort, got {other:?}"),
        }
        assert!(b.aborted_reason().is_some());
        assert!(!b.is_running());
    }

    #[test]
    fn fatal_failure_aborts_immediately() {
        let mut b = Bootstrap::default();
        match b.fail(FailureKind::Fatal, "bad token") {
            Transition::Aborted(msg) => assert!(msg.contains("bad token")),
            other => panic!("expected abort, got {other:?}"),
        }
        // Once aborted, neither success nor failure advances.
        assert!(matches!(b.succeed(), Transition::Aborted(_)));
        assert!(matches!(
            b.fail(FailureKind::Transient, "y"),
            Transition::Aborted(_)
        ));
        assert!(!b.is_running());
    }

    #[test]
    fn retry_budget_resets_each_phase() {
        let mut b = Bootstrap::new(2);
        // Burn one retry at Initializing, then succeed.
        assert!(matches!(
            b.fail(FailureKind::Transient, "x"),
            Transition::Retrying { attempt: 1, .. }
        ));
        b.succeed(); // -> DatastoreReady, budget reset
        // The fresh phase gets its full budget again (attempt 1, not 2).
        assert_eq!(
            b.fail(FailureKind::Transient, "x"),
            Transition::Retrying {
                phase: Phase::DatastoreReady,
                attempt: 1
            }
        );
    }

    #[test]
    fn failure_kind_retryability() {
        assert!(FailureKind::Transient.is_retryable());
        assert!(!FailureKind::Fatal.is_retryable());
    }

    #[test]
    fn phase_display_is_nonempty_for_all() {
        for p in [
            Phase::Initializing,
            Phase::DatastoreReady,
            Phase::ApiserverReady,
            Phase::ControlPlaneReady,
            Phase::NodeReady,
            Phase::Running,
        ] {
            assert!(!format!("{p}").is_empty());
        }
    }
}
