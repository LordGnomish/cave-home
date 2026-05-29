// SPDX-License-Identifier: Apache-2.0
//! Probe state machines — liveness / readiness / startup.
//!
//! Behavioural reimplementation of the documented kubelet prober worker
//! (`pkg/kubelet/prober/worker.go`) threshold-counting logic and the readiness
//! gating that derives a pod's `Ready` condition
//! (`pkg/kubelet/status/generate.go::GeneratePodReadyCondition`).
//!
//! Each probe has a `success_threshold` and a `failure_threshold`. A single
//! probe execution yields a raw [`ProbeOutcome`] (success / failure); the worker
//! accumulates *consecutive* results and only flips the published [`ProbeResult`]
//! once the relevant threshold is crossed:
//!
//! * a run of `failure_threshold` consecutive failures flips the result to
//!   `Failure`;
//! * a run of `success_threshold` consecutive successes flips it to `Success`;
//! * a single opposite-outcome resets the running streak.
//!
//! Probe semantics used by the rest of the kubelet:
//!
//! * **liveness**  — a `Failure` triggers a container restart.
//! * **readiness** — the `Result` is the container's `ready` bit; it gates the
//!   pod `Ready` condition and Service endpoint membership.
//! * **startup**   — while a startup probe has not yet succeeded, the liveness
//!   probe is *suppressed* (startup gates liveness).
//!
//! Pure, `std`-only. The caller drives [`ProbeWorker::record`] once per probe
//! execution; this module performs no scheduling and no I/O.

/// Raw result of a single probe execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeOutcome {
    /// The probe passed this run.
    Success,
    /// The probe failed this run.
    Failure,
}

/// The published (threshold-resolved) probe result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeResult {
    /// Not enough runs yet to publish a verdict.
    Unknown,
    /// The probe is currently passing.
    Success,
    /// The probe is currently failing.
    Failure,
}

/// Probe thresholds (mirrors `v1.Probe.successThreshold` / `failureThreshold`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Thresholds {
    /// Consecutive successes required to flip to [`ProbeResult::Success`].
    pub success_threshold: u32,
    /// Consecutive failures required to flip to [`ProbeResult::Failure`].
    pub failure_threshold: u32,
}

impl Default for Thresholds {
    /// Kubernetes defaults: `successThreshold = 1`, `failureThreshold = 3`.
    fn default() -> Self {
        Self {
            success_threshold: 1,
            failure_threshold: 3,
        }
    }
}

impl Thresholds {
    /// Construct thresholds, clamping each to at least 1 (a zero threshold is
    /// nonsensical and would never flip; the kubelet enforces a minimum of 1).
    #[must_use]
    pub fn new(success_threshold: u32, failure_threshold: u32) -> Self {
        Self {
            success_threshold: success_threshold.max(1),
            failure_threshold: failure_threshold.max(1),
        }
    }
}

/// A threshold-counting probe worker.
#[derive(Clone, Debug)]
pub struct ProbeWorker {
    thresholds: Thresholds,
    result: ProbeResult,
    /// Length of the current consecutive run (of the most recent outcome).
    streak: u32,
    /// Outcome that the current streak is made of (None before the first run).
    streak_of: Option<ProbeOutcome>,
}

impl ProbeWorker {
    /// New worker, result initially [`ProbeResult::Unknown`].
    #[must_use]
    pub const fn new(thresholds: Thresholds) -> Self {
        Self {
            thresholds,
            result: ProbeResult::Unknown,
            streak: 0,
            streak_of: None,
        }
    }

    /// The currently published probe result.
    #[must_use]
    pub const fn result(&self) -> ProbeResult {
        self.result
    }

    /// Record one probe execution and return the (possibly updated) result.
    pub fn record(&mut self, outcome: ProbeOutcome) -> ProbeResult {
        // Maintain the consecutive-run counter.
        if self.streak_of == Some(outcome) {
            self.streak = self.streak.saturating_add(1);
        } else {
            self.streak_of = Some(outcome);
            self.streak = 1;
        }

        match outcome {
            ProbeOutcome::Success => {
                if self.streak >= self.thresholds.success_threshold {
                    self.result = ProbeResult::Success;
                }
            }
            ProbeOutcome::Failure => {
                if self.streak >= self.thresholds.failure_threshold {
                    self.result = ProbeResult::Failure;
                }
            }
        }
        self.result
    }
}

/// Whether a liveness [`ProbeResult`] should trigger a container restart.
///
/// Only a resolved `Failure` triggers a restart; `Unknown` (not enough data)
/// and `Success` do not.
#[must_use]
pub fn liveness_triggers_restart(liveness: ProbeResult) -> bool {
    liveness == ProbeResult::Failure
}

/// Whether the liveness probe is currently *active*, given the startup probe.
///
/// The startup probe gates liveness: liveness runs only once startup has
/// succeeded. `startup == None` means no startup probe is configured, so
/// liveness is always active.
#[must_use]
pub const fn liveness_active(startup: Option<ProbeResult>) -> bool {
    matches!(startup, None | Some(ProbeResult::Success))
}

/// A pod readiness gate (`v1.PodReadinessGate`) — a custom condition the pod
/// author requires before the pod is considered Ready.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadinessGate {
    /// The condition type that must be `True`.
    pub condition_type: String,
    /// Whether that condition is currently `True`.
    pub status_true: bool,
}

/// Derive the pod `Ready` condition.
///
/// The pod is Ready iff **every** container's readiness probe result is
/// `Success` (containers with no readiness probe are Ready by default — the
/// caller passes their result as `Success`) **and** every declared readiness
/// gate's condition is `True`.
///
/// An empty `container_readiness` slice with no gates means the pod is Ready
/// (degenerate: nothing blocks readiness).
#[must_use]
pub fn pod_ready(container_readiness: &[ProbeResult], gates: &[ReadinessGate]) -> bool {
    let all_containers_ready = container_readiness
        .iter()
        .all(|r| *r == ProbeResult::Success);
    let all_gates_true = gates.iter().all(|g| g.status_true);
    all_containers_ready && all_gates_true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_one_and_three() {
        let t = Thresholds::default();
        assert_eq!(t.success_threshold, 1);
        assert_eq!(t.failure_threshold, 3);
    }

    #[test]
    fn new_thresholds_clamp_zero_to_one() {
        let t = Thresholds::new(0, 0);
        assert_eq!(t.success_threshold, 1);
        assert_eq!(t.failure_threshold, 1);
    }

    #[test]
    fn starts_unknown() {
        let w = ProbeWorker::new(Thresholds::default());
        assert_eq!(w.result(), ProbeResult::Unknown);
    }

    #[test]
    fn single_success_with_default_flips_to_success() {
        let mut w = ProbeWorker::new(Thresholds::default());
        assert_eq!(w.record(ProbeOutcome::Success), ProbeResult::Success);
    }

    #[test]
    fn failure_threshold_three_needs_three_consecutive() {
        let mut w = ProbeWorker::new(Thresholds::new(1, 3));
        w.record(ProbeOutcome::Success); // Success
        assert_eq!(w.record(ProbeOutcome::Failure), ProbeResult::Success); // 1 fail
        assert_eq!(w.record(ProbeOutcome::Failure), ProbeResult::Success); // 2 fail
        assert_eq!(w.record(ProbeOutcome::Failure), ProbeResult::Failure); // 3 fail -> flip
    }

    #[test]
    fn a_success_resets_the_failure_streak() {
        let mut w = ProbeWorker::new(Thresholds::new(1, 3));
        w.record(ProbeOutcome::Failure);
        w.record(ProbeOutcome::Failure); // 2 fails, not yet flipped
        w.record(ProbeOutcome::Success); // resets streak, flips to Success (thr 1)
        assert_eq!(w.result(), ProbeResult::Success);
        // Now we need a fresh run of 3 to fail again.
        w.record(ProbeOutcome::Failure);
        w.record(ProbeOutcome::Failure);
        assert_eq!(w.result(), ProbeResult::Success); // still only 2
        assert_eq!(w.record(ProbeOutcome::Failure), ProbeResult::Failure);
    }

    #[test]
    fn success_threshold_greater_than_one() {
        // Readiness with successThreshold=2: needs two consecutive successes.
        let mut w = ProbeWorker::new(Thresholds::new(2, 1));
        assert_eq!(w.record(ProbeOutcome::Success), ProbeResult::Unknown); // 1 success
        assert_eq!(w.record(ProbeOutcome::Success), ProbeResult::Success); // 2 -> flip
    }

    #[test]
    fn failure_resets_success_streak() {
        let mut w = ProbeWorker::new(Thresholds::new(2, 1));
        w.record(ProbeOutcome::Success); // 1 success
        // single failure (threshold 1) flips immediately to Failure
        assert_eq!(w.record(ProbeOutcome::Failure), ProbeResult::Failure);
        // need 2 successes again to recover
        assert_eq!(w.record(ProbeOutcome::Success), ProbeResult::Failure);
        assert_eq!(w.record(ProbeOutcome::Success), ProbeResult::Success);
    }

    #[test]
    fn liveness_failure_triggers_restart() {
        assert!(liveness_triggers_restart(ProbeResult::Failure));
        assert!(!liveness_triggers_restart(ProbeResult::Success));
        assert!(!liveness_triggers_restart(ProbeResult::Unknown));
    }

    #[test]
    fn startup_gates_liveness() {
        // No startup probe -> liveness always active.
        assert!(liveness_active(None));
        // Startup not yet succeeded -> liveness suppressed.
        assert!(!liveness_active(Some(ProbeResult::Unknown)));
        assert!(!liveness_active(Some(ProbeResult::Failure)));
        // Startup succeeded -> liveness active.
        assert!(liveness_active(Some(ProbeResult::Success)));
    }

    #[test]
    fn pod_ready_requires_all_containers_ready() {
        assert!(pod_ready(&[ProbeResult::Success, ProbeResult::Success], &[]));
        assert!(!pod_ready(&[ProbeResult::Success, ProbeResult::Failure], &[]));
        assert!(!pod_ready(&[ProbeResult::Unknown], &[]));
    }

    #[test]
    fn pod_ready_empty_is_ready() {
        assert!(pod_ready(&[], &[]));
    }

    #[test]
    fn readiness_gates_must_be_true() {
        let ready_containers = [ProbeResult::Success];
        let gate_false = ReadinessGate {
            condition_type: "www.example.com/feature-1".into(),
            status_true: false,
        };
        let gate_true = ReadinessGate {
            condition_type: "www.example.com/feature-1".into(),
            status_true: true,
        };
        assert!(!pod_ready(&ready_containers, std::slice::from_ref(&gate_false)));
        assert!(pod_ready(&ready_containers, std::slice::from_ref(&gate_true)));
    }

    #[test]
    fn readiness_gate_cannot_override_unready_container() {
        let gate_true = ReadinessGate {
            condition_type: "x".into(),
            status_true: true,
        };
        // Container not ready -> pod not ready even with all gates true.
        assert!(!pod_ready(&[ProbeResult::Failure], std::slice::from_ref(&gate_true)));
    }
}
