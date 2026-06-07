// SPDX-License-Identifier: Apache-2.0
//! Failing tests (RED) for the ReplicaSet controller decision core.
//!
//! Behavioural reference: kubernetes/kubernetes `pkg/controller/replicaset`
//! (`replica_set.go::manageReplicas`) and the shared victim ordering in
//! `pkg/controller/controller_utils.go` (`ActivePodsWithRanks.Less` /
//! `getPodsToDelete`). Pure decision over caller-supplied pods — no client,
//! no clock, no I/O.
//!
//! The contract under test:
//!   * count *active* pods that match the ReplicaSet selector (active =
//!     phase ∉ {Succeeded, Failed} and not terminating);
//!   * `diff = active − desired`;
//!   * `diff < 0` → create `−diff` pods;
//!   * `diff > 0` → delete `diff` pods, choosing victims by the documented
//!     deletion preference (unassigned, then less-ready phase, then not-ready,
//!     then lower pod-deletion-cost, then higher restart count, then younger);
//!   * `diff == 0` → in sync.

use cave_home_controller_manager_rs::controllers::replicaset::{
    PodPhase, PodView, ReplicaSetAction, ReplicaSetSpec, reconcile,
};

fn spec(replicas: i32) -> ReplicaSetSpec {
    ReplicaSetSpec::new(replicas).select("app", "web")
}

/// A healthy, assigned, ready pod that matches `spec("web")`.
fn web(uid: &str) -> PodView {
    PodView::running(uid).with_label("app", "web")
}

#[test]
fn scale_up_creates_the_missing_pods() {
    // 1 active, want 3 → create 2.
    let pods = [web("a")];
    assert_eq!(reconcile(&spec(3), &pods), ReplicaSetAction::CreatePods(2));
}

#[test]
fn scale_up_from_zero_creates_all() {
    assert_eq!(reconcile(&spec(3), &[]), ReplicaSetAction::CreatePods(3));
}

#[test]
fn in_sync_when_active_equals_desired() {
    let pods = [web("a"), web("b")];
    assert_eq!(reconcile(&spec(2), &pods), ReplicaSetAction::InSync);
}

#[test]
fn scale_down_deletes_the_excess() {
    let pods = [web("a"), web("b"), web("c"), web("d")];
    match reconcile(&spec(2), &pods) {
        ReplicaSetAction::DeletePods(v) => assert_eq!(v.len(), 2),
        other => panic!("expected DeletePods(2), got {other:?}"),
    }
}

#[test]
fn only_selector_matching_pods_are_counted() {
    // Two matching + one non-matching; want 3 → still need to create 1.
    let pods = [
        web("a"),
        web("b"),
        PodView::running("other").with_label("app", "db"),
    ];
    assert_eq!(reconcile(&spec(3), &pods), ReplicaSetAction::CreatePods(1));
}

#[test]
fn failed_and_succeeded_pods_are_not_active() {
    let pods = [
        web("a"),
        web("b").with_phase(PodPhase::Failed),
        web("c").with_phase(PodPhase::Succeeded),
    ];
    // Only "a" is active; want 2 → create 1.
    assert_eq!(reconcile(&spec(2), &pods), ReplicaSetAction::CreatePods(1));
}

#[test]
fn terminating_pods_are_not_active() {
    let pods = [web("a"), web("b").terminating_at(500)];
    // "b" has a deletion timestamp → not active; want 2 → create 1.
    assert_eq!(reconcile(&spec(2), &pods), ReplicaSetAction::CreatePods(1));
}

#[test]
fn negative_replicas_treated_as_zero_deletes_all_active() {
    let pods = [web("a"), web("b")];
    match reconcile(&ReplicaSetSpec::new(-5).select("app", "web"), &pods) {
        ReplicaSetAction::DeletePods(v) => assert_eq!(v.len(), 2),
        other => panic!("expected DeletePods(2), got {other:?}"),
    }
}

// ---- victim ordering (getPodsToDelete) -------------------------------------

#[test]
fn scale_down_prefers_unassigned_pods() {
    // "pending-unassigned" has no node → deleted before the assigned one.
    let pods = [
        web("assigned").on_node("n1"),
        web("unassigned").unassigned(),
    ];
    assert_eq!(
        reconcile(&spec(1), &pods),
        ReplicaSetAction::DeletePods(vec!["unassigned".to_owned()])
    );
}

#[test]
fn scale_down_prefers_less_ready_phase() {
    // Pending (ordinal 0) is deleted before Running (ordinal 2).
    let pods = [
        web("running").with_phase(PodPhase::Running),
        web("pending").with_phase(PodPhase::Pending),
    ];
    assert_eq!(
        reconcile(&spec(1), &pods),
        ReplicaSetAction::DeletePods(vec!["pending".to_owned()])
    );
}

#[test]
fn scale_down_prefers_not_ready_over_ready() {
    let pods = [web("ready"), web("notready").not_ready()];
    assert_eq!(
        reconcile(&spec(1), &pods),
        ReplicaSetAction::DeletePods(vec!["notready".to_owned()])
    );
}

#[test]
fn scale_down_prefers_lower_deletion_cost() {
    let pods = [
        web("keep").with_deletion_cost(100),
        web("cheap").with_deletion_cost(-10),
    ];
    assert_eq!(
        reconcile(&spec(1), &pods),
        ReplicaSetAction::DeletePods(vec!["cheap".to_owned()])
    );
}

#[test]
fn scale_down_prefers_higher_restart_count() {
    let pods = [
        web("stable").with_restarts(0),
        web("flapping").with_restarts(7),
    ];
    assert_eq!(
        reconcile(&spec(1), &pods),
        ReplicaSetAction::DeletePods(vec!["flapping".to_owned()])
    );
}

#[test]
fn scale_down_prefers_younger_pod() {
    // Later creation timestamp = younger = deleted first.
    let pods = [web("old").created_at(100), web("young").created_at(900)];
    assert_eq!(
        reconcile(&spec(1), &pods),
        ReplicaSetAction::DeletePods(vec!["young".to_owned()])
    );
}

#[test]
fn deletion_preference_is_ordered_unassigned_before_phase() {
    // Unassigned-but-Running must still outrank assigned-Pending: the
    // unassigned key dominates the phase key.
    let pods = [
        web("assigned-pending")
            .on_node("n1")
            .with_phase(PodPhase::Pending),
        web("unassigned-running")
            .unassigned()
            .with_phase(PodPhase::Running),
    ];
    assert_eq!(
        reconcile(&spec(1), &pods),
        ReplicaSetAction::DeletePods(vec!["unassigned-running".to_owned()])
    );
}
