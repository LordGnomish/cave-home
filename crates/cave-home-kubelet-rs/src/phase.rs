// SPDX-License-Identifier: Apache-2.0
//! Pod-phase derivation — the core kubelet status decision.
//!
//! Behavioural reimplementation of the documented kubelet pod-phase algorithm
//! (`generateAPIPodStatus` -> `getPhase` in `pkg/kubelet/kubelet_pods.go`). This
//! is the *truth table* that maps the per-container states (waiting / running /
//! terminated + exit codes) and the pod `RestartPolicy` onto the high-level
//! [`PodPhase`] reported to the control plane.
//!
//! The algorithm (per the Kubernetes "Pod Lifecycle" documentation):
//!
//! * **Pending** — the pod has been admitted but one or more containers have
//!   not started (still waiting), and nothing has terminally failed.
//! * **Running** — at least one container is running, or is in the process of
//!   starting/restarting, and the pod has not reached a terminal phase.
//! * **Succeeded** — all containers terminated **successfully** (exit 0) and
//!   will not be restarted (governed by [`RestartPolicy`]).
//! * **Failed** — all containers have terminated and **at least one** failed
//!   (non-zero exit) in a way that will not be restarted.
//!
//! The restart policy gates whether a non-zero exit is *terminal*:
//!
//! * `Never`     — any non-zero exit is terminal -> contributes to `Failed`;
//!                 a zero exit is terminal success.
//! * `OnFailure` — a non-zero exit is *not* terminal (it will be restarted, so
//!                 the container is effectively still "running"); only a zero
//!                 exit is terminal success.
//! * `Always`    — *no* termination is terminal; a terminated container is
//!                 always going to be restarted, so the pod stays `Running`.
//!
//! Pure, `std`-only, total function: no panics, no I/O, no clock.

use crate::api::{ContainerState, ContainerStatus, PodPhase, RestartPolicy};

/// Per-container terminal classification under a given restart policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ContainerOutcome {
    /// Waiting to start (no running/terminated state yet).
    Waiting,
    /// Currently running (includes "will be restarted" terminations, which
    /// the kubelet treats as not-yet-terminal).
    Running,
    /// Terminated and will NOT be restarted, exit code 0.
    Succeeded,
    /// Terminated and will NOT be restarted, non-zero exit code.
    Failed,
}

const fn classify(status: &ContainerStatus, policy: RestartPolicy) -> ContainerOutcome {
    match &status.state {
        ContainerState::Waiting(_) => ContainerOutcome::Waiting,
        ContainerState::Running(_) => ContainerOutcome::Running,
        ContainerState::Terminated(t) => {
            let success = t.exit_code == 0;
            match policy {
                // Always: every termination is followed by a restart, so the
                // container never reaches a terminal pod-phase contribution.
                RestartPolicy::Always => ContainerOutcome::Running,
                // OnFailure: success is terminal, failure restarts.
                RestartPolicy::OnFailure => {
                    if success {
                        ContainerOutcome::Succeeded
                    } else {
                        ContainerOutcome::Running
                    }
                }
                // Never: the termination is terminal either way.
                RestartPolicy::Never => {
                    if success {
                        ContainerOutcome::Succeeded
                    } else {
                        ContainerOutcome::Failed
                    }
                }
            }
        }
    }
}

/// Derive the [`PodPhase`] from the per-container statuses and restart policy.
///
/// `expected` is the number of containers declared in the pod spec; if fewer
/// container statuses are present than expected, the missing containers count
/// as still `Waiting` (the kubelet has not observed them start yet), which keeps
/// the pod `Pending`/`Running` rather than prematurely terminal.
///
/// A pod with no containers at all reports [`PodPhase::Succeeded`] (a degenerate
/// but well-defined case: nothing left to run).
///
/// # Examples
///
/// ```
/// use cave_home_kubelet_rs::api::{
///     ContainerState, ContainerStateTerminated, ContainerStatus, PodPhase, RestartPolicy,
/// };
/// use cave_home_kubelet_rs::phase::derive_phase;
///
/// let failed = ContainerStatus {
///     name: "app".into(),
///     state: ContainerState::Terminated(ContainerStateTerminated {
///         exit_code: 1,
///         ..Default::default()
///     }),
///     ..Default::default()
/// };
/// // Never + a non-zero exit -> the pod has Failed.
/// assert_eq!(derive_phase(&[failed], 1, RestartPolicy::Never), PodPhase::Failed);
/// ```
#[must_use]
pub fn derive_phase(
    statuses: &[ContainerStatus],
    expected: usize,
    policy: RestartPolicy,
) -> PodPhase {
    let observed = statuses.len();
    // Containers declared in the spec but not yet observed count as Waiting.
    let pending_unobserved = expected.saturating_sub(observed);

    let mut waiting = pending_unobserved;
    let mut running = 0usize;
    let mut succeeded = 0usize;
    let mut failed = 0usize;

    for s in statuses {
        match classify(s, policy) {
            ContainerOutcome::Waiting => waiting += 1,
            ContainerOutcome::Running => running += 1,
            ContainerOutcome::Succeeded => succeeded += 1,
            ContainerOutcome::Failed => failed += 1,
        }
    }

    let total = waiting + running + succeeded + failed;
    if total == 0 {
        // No containers in the pod at all — degenerate Succeeded.
        return PodPhase::Succeeded;
    }

    // Anything still running (or scheduled to restart) keeps the pod Running.
    if running > 0 {
        return PodPhase::Running;
    }

    // Nothing running. If anything is still waiting to start, the pod is
    // either Pending (nothing has run yet) or Running (some already finished
    // but others are mid-start). Upstream treats "some succeeded, some still
    // waiting" as Pending only when *nothing* has progressed.
    if waiting > 0 {
        // If we've already seen terminal results for some containers but others
        // are waiting, the pod is still making progress -> Running. If literally
        // everything is waiting, it is Pending.
        if succeeded == 0 && failed == 0 {
            return PodPhase::Pending;
        }
        return PodPhase::Running;
    }

    // Every container has reached a terminal outcome.
    if failed > 0 {
        PodPhase::Failed
    } else {
        PodPhase::Succeeded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{
        ContainerStateRunning, ContainerStateTerminated, ContainerStateWaiting,
    };

    fn waiting(name: &str) -> ContainerStatus {
        ContainerStatus {
            name: name.into(),
            state: ContainerState::Waiting(ContainerStateWaiting::default()),
            ..Default::default()
        }
    }

    fn running(name: &str) -> ContainerStatus {
        ContainerStatus {
            name: name.into(),
            state: ContainerState::Running(ContainerStateRunning::default()),
            ..Default::default()
        }
    }

    fn terminated(name: &str, exit: i32) -> ContainerStatus {
        ContainerStatus {
            name: name.into(),
            state: ContainerState::Terminated(ContainerStateTerminated {
                exit_code: exit,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn empty_pod_is_succeeded() {
        assert_eq!(derive_phase(&[], 0, RestartPolicy::Always), PodPhase::Succeeded);
    }

    #[test]
    fn all_waiting_is_pending() {
        let cs = [waiting("a"), waiting("b")];
        assert_eq!(derive_phase(&cs, 2, RestartPolicy::Always), PodPhase::Pending);
        assert_eq!(derive_phase(&cs, 2, RestartPolicy::Never), PodPhase::Pending);
    }

    #[test]
    fn unobserved_containers_keep_pod_pending() {
        // 1 status present (waiting), spec declares 2 -> still all waiting.
        let cs = [waiting("a")];
        assert_eq!(derive_phase(&cs, 2, RestartPolicy::Always), PodPhase::Pending);
    }

    #[test]
    fn any_running_is_running() {
        let cs = [running("a"), waiting("b")];
        assert_eq!(derive_phase(&cs, 2, RestartPolicy::Never), PodPhase::Running);
    }

    #[test]
    fn never_any_failed_is_failed() {
        let cs = [terminated("a", 0), terminated("b", 137)];
        assert_eq!(derive_phase(&cs, 2, RestartPolicy::Never), PodPhase::Failed);
    }

    #[test]
    fn never_all_succeeded_is_succeeded() {
        let cs = [terminated("a", 0), terminated("b", 0)];
        assert_eq!(derive_phase(&cs, 2, RestartPolicy::Never), PodPhase::Succeeded);
    }

    #[test]
    fn onfailure_all_succeeded_is_succeeded() {
        let cs = [terminated("a", 0), terminated("b", 0)];
        assert_eq!(derive_phase(&cs, 2, RestartPolicy::OnFailure), PodPhase::Succeeded);
    }

    #[test]
    fn onfailure_with_failure_stays_running_pending_restart() {
        // OnFailure: a non-zero exit will be restarted -> treated as Running.
        let cs = [terminated("a", 0), terminated("b", 1)];
        assert_eq!(derive_phase(&cs, 2, RestartPolicy::OnFailure), PodPhase::Running);
    }

    #[test]
    fn always_terminated_zero_is_running_pending_restart() {
        // Always: even a clean exit will be restarted -> Running, never Succeeded.
        let cs = [terminated("a", 0)];
        assert_eq!(derive_phase(&cs, 1, RestartPolicy::Always), PodPhase::Running);
    }

    #[test]
    fn always_terminated_nonzero_is_running_pending_restart() {
        let cs = [terminated("a", 5)];
        assert_eq!(derive_phase(&cs, 1, RestartPolicy::Always), PodPhase::Running);
    }

    #[test]
    fn never_single_success_is_succeeded() {
        let cs = [terminated("a", 0)];
        assert_eq!(derive_phase(&cs, 1, RestartPolicy::Never), PodPhase::Succeeded);
    }

    #[test]
    fn never_single_failure_is_failed() {
        let cs = [terminated("a", 1)];
        assert_eq!(derive_phase(&cs, 1, RestartPolicy::Never), PodPhase::Failed);
    }

    #[test]
    fn never_mixed_succeeded_and_waiting_is_running() {
        // One done (success), one still starting -> the pod is progressing.
        let cs = [terminated("a", 0), waiting("b")];
        assert_eq!(derive_phase(&cs, 2, RestartPolicy::Never), PodPhase::Running);
    }

    #[test]
    fn never_failed_and_waiting_is_running_not_failed_yet() {
        // One failed terminally, one still waiting -> not all terminal -> Running.
        let cs = [terminated("a", 2), waiting("b")];
        assert_eq!(derive_phase(&cs, 2, RestartPolicy::Never), PodPhase::Running);
    }

    #[test]
    fn onfailure_failed_container_does_not_make_pod_failed() {
        // The classic difference vs Never: OnFailure never reports Failed for a
        // restartable container.
        let cs = [terminated("a", 99)];
        assert_eq!(derive_phase(&cs, 1, RestartPolicy::OnFailure), PodPhase::Running);
        assert_eq!(derive_phase(&cs, 1, RestartPolicy::Never), PodPhase::Failed);
    }
}
