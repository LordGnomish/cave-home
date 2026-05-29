// SPDX-License-Identifier: Apache-2.0
//! Pod-status sync — the pure desired-vs-observed action diff.
//!
//! Behavioural reimplementation of the *decision* half of
//! `pkg/kubelet/kuberuntime/kuberuntime_manager.go::computePodActions`: given the
//! desired pod spec and the observed container states, compute the set of
//! [`SyncAction`]s (start / kill a container) the runtime should perform. This is
//! the side-effect-free core that the async [`crate::podworker`] executes against
//! the CRI; isolating it makes the lifecycle decisions exhaustively testable.
//!
//! Rules (Phase 1 subset of `computePodActions`):
//!
//! * A desired container with **no observed status** -> `Start` (initial run).
//! * A desired container observed **running** -> no action.
//! * A desired container observed **terminated** -> `Start` iff the restart
//!   policy + exit code permit it (see [`crate::restart::should_restart`]),
//!   otherwise no action.
//! * A desired container observed **waiting** -> no action (the runtime is
//!   already bringing it up).
//! * An **observed running** container that is **no longer in the spec** ->
//!   `Kill` (it was removed from the pod).
//!
//! Pure, `std`-only.

use crate::api::{ContainerState, ContainerStatus, PodSpec};
use crate::restart::should_restart;

/// An action the runtime should take to converge a container toward the spec.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyncAction {
    /// Create + start the named container.
    Start(String),
    /// Stop the named container (no longer desired).
    Kill(String),
}

/// Compute the ordered set of [`SyncAction`]s to converge `observed` toward
/// `desired`.
///
/// `Start` actions are emitted in spec order first, then `Kill` actions in
/// observed order, so the output is deterministic.
#[must_use]
pub fn compute_actions(desired: &PodSpec, observed: &[ContainerStatus]) -> Vec<SyncAction> {
    let mut actions = Vec::new();

    // Starts (and restart-or-not decisions) in spec order.
    for c in &desired.containers {
        match observed.iter().find(|s| s.name == c.name) {
            None => actions.push(SyncAction::Start(c.name.clone())),
            Some(status) => match &status.state {
                ContainerState::Running(_) | ContainerState::Waiting(_) => {}
                ContainerState::Terminated(t) => {
                    if should_restart(desired.restart_policy, t.exit_code) {
                        actions.push(SyncAction::Start(c.name.clone()));
                    }
                }
            },
        }
    }

    // Kills for observed-running containers that left the spec.
    for status in observed {
        let still_desired = desired.containers.iter().any(|c| c.name == status.name);
        if !still_desired && matches!(status.state, ContainerState::Running(_)) {
            actions.push(SyncAction::Kill(status.name.clone()));
        }
    }

    actions
}

/// Convenience: are `desired` and `observed` already converged (no actions)?
#[must_use]
pub fn is_converged(desired: &PodSpec, observed: &[ContainerStatus]) -> bool {
    compute_actions(desired, observed).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{
        Container, ContainerStateRunning, ContainerStateTerminated, ContainerStateWaiting,
        RestartPolicy,
    };

    fn spec(names: &[&str], policy: RestartPolicy) -> PodSpec {
        PodSpec {
            containers: names
                .iter()
                .map(|n| Container {
                    name: (*n).into(),
                    image: "img".into(),
                    ..Default::default()
                })
                .collect(),
            restart_policy: policy,
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

    fn waiting(name: &str) -> ContainerStatus {
        ContainerStatus {
            name: name.into(),
            state: ContainerState::Waiting(ContainerStateWaiting::default()),
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
    fn fresh_pod_starts_all_containers() {
        let s = spec(&["a", "b"], RestartPolicy::Always);
        let actions = compute_actions(&s, &[]);
        assert_eq!(
            actions,
            vec![SyncAction::Start("a".into()), SyncAction::Start("b".into())]
        );
    }

    #[test]
    fn running_containers_need_no_action() {
        let s = spec(&["a"], RestartPolicy::Always);
        assert!(is_converged(&s, &[running("a")]));
    }

    #[test]
    fn waiting_container_needs_no_action() {
        let s = spec(&["a"], RestartPolicy::Always);
        assert!(is_converged(&s, &[waiting("a")]));
    }

    #[test]
    fn terminated_with_always_restarts() {
        let s = spec(&["a"], RestartPolicy::Always);
        assert_eq!(
            compute_actions(&s, &[terminated("a", 0)]),
            vec![SyncAction::Start("a".into())]
        );
    }

    #[test]
    fn terminated_clean_with_onfailure_does_not_restart() {
        let s = spec(&["a"], RestartPolicy::OnFailure);
        assert!(is_converged(&s, &[terminated("a", 0)]));
    }

    #[test]
    fn terminated_failed_with_onfailure_restarts() {
        let s = spec(&["a"], RestartPolicy::OnFailure);
        assert_eq!(
            compute_actions(&s, &[terminated("a", 1)]),
            vec![SyncAction::Start("a".into())]
        );
    }

    #[test]
    fn terminated_with_never_does_not_restart() {
        let s = spec(&["a"], RestartPolicy::Never);
        assert!(is_converged(&s, &[terminated("a", 1)]));
        assert!(is_converged(&s, &[terminated("a", 0)]));
    }

    #[test]
    fn container_removed_from_spec_is_killed() {
        let s = spec(&["a"], RestartPolicy::Always);
        let observed = [running("a"), running("ghost")];
        assert_eq!(
            compute_actions(&s, &observed),
            vec![SyncAction::Kill("ghost".into())]
        );
    }

    #[test]
    fn terminated_ghost_is_not_killed() {
        // Only running ghosts need a Kill; an already-exited ghost needs nothing.
        let s = spec(&["a"], RestartPolicy::Always);
        let observed = [running("a"), terminated("ghost", 0)];
        assert!(is_converged(&s, &observed));
    }

    #[test]
    fn combined_start_and_kill_is_deterministic() {
        let s = spec(&["a", "b"], RestartPolicy::Always);
        // a is missing (start), b running (skip), ghost running (kill).
        let observed = [running("b"), running("ghost")];
        let actions = compute_actions(&s, &observed);
        assert_eq!(
            actions,
            vec![SyncAction::Start("a".into()), SyncAction::Kill("ghost".into())]
        );
    }
}
