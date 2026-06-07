// SPDX-License-Identifier: Apache-2.0
//! `ReplicaSet` controller — drives observed pod count toward `spec.replicas`.
//!
//! Behavioural reimplementation of the documented `pkg/controller/replicaset`
//! contract, reconciling against the in-memory apiserver
//! ([`crate::apis::Cluster`]):
//!
//! * **adopt** matching pods that have no controller (set a controller +
//!   `blockOwnerDeletion` owner reference), and **release** owned pods that no
//!   longer match the selector (clear the owner reference) — the
//!   `claimPods`/`ReleaseControllerRef` contract;
//! * count **active** owned pods (not terminal, not terminating) and create or
//!   delete the difference against `spec.replicas`;
//! * when scaling down, delete the **least valuable** pods first
//!   (`getPodsToDelete` / `ActivePodsWithRanks.Less`): lower phase, then
//!   not-ready, then newer pods;
//! * write `status.replicas` / `ready_replicas` / `available_replicas` back.
//!
//! No I/O: every read and write goes through the [`Cluster`] handle, which a
//! real run loop would back with a networked clientset.

use crate::apis::{Cluster, Pod, PodPhase, ReplicaSet};
use crate::reconcile::Outcome;
use crate::types::{Object, ObjectMeta, OwnerReference};

/// The `ReplicaSet` controller. Holds a monotonic counter so the pods it
/// creates get unique, stable names (`<rs-name>-<seq36>`), the deterministic
/// analogue of upstream's random `generateName` suffix.
#[derive(Debug, Default)]
pub struct ReplicaSetController {
    pod_seq: u64,
}

impl ReplicaSetController {
    /// A fresh controller.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reconcile one `ReplicaSet` key (`"<ns>/<name>"`).
    ///
    /// Returns [`Outcome::Done`] on success (including when the RS is absent or
    /// terminating — there is nothing for this controller to do then).
    pub fn reconcile(&mut self, key: &str, cluster: &mut Cluster, _now: u64) -> Outcome {
        let Some(rs) = cluster.replicasets.get(key) else {
            return Outcome::Done; // RS deleted; nothing to reconcile.
        };
        if rs.meta.is_terminating() {
            return Outcome::Done; // GC drives teardown of a terminating RS.
        }

        claim_and_release(&rs, cluster);

        // Recount owned pods after adoption/release.
        let owned = cluster.pods.list_owned_by(&rs.meta.uid);
        let active: Vec<Pod> = owned.iter().filter(|p| p.is_active()).cloned().collect();
        let diff = clamp_i32(active.len()) - rs.spec.replicas;

        if diff < 0 {
            self.create_pods(&rs, cluster, diff.unsigned_abs() as usize);
        } else if diff > 0 {
            delete_surplus(&active, cluster, diff.unsigned_abs() as usize);
        }

        write_status(&rs, cluster);
        Outcome::Done
    }

    /// Create `count` new pods from the RS template.
    fn create_pods(&mut self, rs: &ReplicaSet, cluster: &mut Cluster, count: usize) {
        for _ in 0..count {
            self.pod_seq += 1;
            let name = format!("{}-{}", rs.meta.name, base36(self.pod_seq));
            let mut meta = ObjectMeta::new(&name, &rs.meta.namespace, "");
            meta.labels = rs.spec.template.labels.clone();
            meta.owner_references = vec![OwnerReference::to(
                "ReplicaSet",
                &rs.meta.name,
                &rs.meta.uid,
            )
            .controller()
            .blocking()];
            cluster.pods.create(Pod::new(meta));
        }
    }
}

/// Adopt matching orphans and release drifted children (free function: it has
/// no controller state, only the RS and the cluster).
fn claim_and_release(rs: &ReplicaSet, cluster: &mut Cluster) {
    let ns = &rs.meta.namespace;
    // Adopt: pods in-namespace matching the selector with no controller.
    for pod in cluster.pods.list_matching(ns, &rs.spec.selector) {
        let has_controller = pod.meta.owner_references.iter().any(|r| r.controller);
        if !has_controller {
            let mut adopted = pod;
            adopted.meta.owner_references.push(
                OwnerReference::to("ReplicaSet", &rs.meta.name, &rs.meta.uid)
                    .controller()
                    .blocking(),
            );
            cluster.pods.update(adopted);
        }
    }
    // Release: pods we control that no longer match the selector.
    for pod in cluster.pods.list_owned_by(&rs.meta.uid) {
        let matches = crate::apis::selector_matches(&rs.spec.selector, &pod.meta.labels);
        if !matches {
            let mut released = pod;
            released
                .meta
                .owner_references
                .retain(|r| !(r.controller && r.uid == rs.meta.uid));
            cluster.pods.update(released);
        }
    }
}

/// Delete `count` of the least-valuable active pods (scale-down victims).
fn delete_surplus(active: &[Pod], cluster: &mut Cluster, count: usize) {
    let mut victims: Vec<Pod> = active.to_vec();
    victims.sort_by_key(victim_rank);
    for pod in victims.into_iter().take(count) {
        cluster.pods.delete(&pod.key());
    }
}

/// Lower rank = deleted first (upstream `ActivePodsWithRanks.Less`, the subset
/// this model carries):
/// 1. phase ordering: `Pending` (0) < `Unknown` (1) < `Running` (2) — weaker
///    pods go first;
/// 2. not-ready before ready;
/// 3. newer pods before older — approximated by a higher UID suffix, so the
///    most recently created pod is the first victim.
fn victim_rank(p: &Pod) -> (u8, u8, std::cmp::Reverse<String>) {
    let phase = match p.status.phase {
        PodPhase::Unknown => 1,
        PodPhase::Running => 2,
        // Pending goes first; terminal pods never appear in the active set but
        // rank lowest alongside Pending.
        PodPhase::Pending | PodPhase::Succeeded | PodPhase::Failed => 0,
    };
    let ready = u8::from(p.status.ready); // not-ready (0) before ready (1)
    (phase, ready, std::cmp::Reverse(p.meta.uid.clone()))
}

/// Recompute and persist the RS status from its currently-owned pods.
fn write_status(rs: &ReplicaSet, cluster: &mut Cluster) {
    let owned = cluster.pods.list_owned_by(&rs.meta.uid);
    let active: Vec<&Pod> = owned.iter().filter(|p| p.is_active()).collect();
    let replicas = clamp_i32(active.len());
    let ready = clamp_i32(active.iter().filter(|p| p.status.ready).count());
    if let Some(mut current) = cluster.replicasets.get(&rs.key()) {
        current.status.replicas = replicas;
        current.status.ready_replicas = ready;
        current.status.available_replicas = ready;
        cluster.replicasets.update(current);
    }
}

/// Saturating `usize -> i32` for replica counts (counts never realistically
/// exceed `i32::MAX`, but this keeps the arithmetic lossless and lint-clean).
fn clamp_i32(n: usize) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}

/// Render `n` as a short base36 string (lowercase alphanumeric).
fn base36(mut n: u64) -> String {
    const ALPHABET: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".to_owned();
    }
    let mut out = Vec::new();
    while n > 0 {
        out.push(ALPHABET[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}
