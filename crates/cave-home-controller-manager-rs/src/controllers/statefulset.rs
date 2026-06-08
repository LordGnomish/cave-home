// SPDX-License-Identifier: Apache-2.0
//! `StatefulSet` controller — ordered, stably-named replicas with stable
//! per-ordinal storage.
//!
//! Behavioural reimplementation of the documented `pkg/controller/statefulset`
//! contract, reconciling against the in-memory apiserver:
//!
//! * pods are named `<sts>-<ordinal>` for ordinals `0..spec.replicas`;
//! * **`OrderedReady`** (default): scale up creates the lowest missing ordinal
//!   only once every lower-ordinal pod is `Running` + ready (one step per
//!   reconcile); scale down deletes the highest ordinal at or above
//!   `spec.replicas`, one per reconcile, highest first;
//! * **`Parallel`**: create every missing ordinal and delete every surplus one
//!   in a single reconcile, with no readiness gating;
//! * each `volumeClaimTemplate` instantiates one PVC per ordinal, named
//!   `<template>-<sts>-<ordinal>`, created **before** (or with) its pod and
//!   **retained** across scale-down — the persistent-identity guarantee;
//! * `status` reports `replicas` / `ready_replicas` / `current_replicas` /
//!   `updated_replicas`.
//!
//! This preserves the ordered-identity and stable-storage guarantees that
//! distinguish a `StatefulSet` from a `ReplicaSet`.

use crate::apis::{Cluster, PersistentVolumeClaim, Pod, PodManagementPolicy, StatefulSet};
use crate::reconcile::Outcome;
use crate::types::{Object, ObjectMeta, OwnerReference};

/// The `StatefulSet` controller.
#[derive(Debug, Default)]
pub struct StatefulSetController;

impl StatefulSetController {
    /// A fresh controller.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Reconcile one `StatefulSet` key (`"<ns>/<name>"`).
    pub fn reconcile(&mut self, key: &str, cluster: &mut Cluster, _now: u64) -> Outcome {
        let Some(sts) = cluster.statefulsets.get(key) else {
            return Outcome::Done;
        };
        if sts.meta.is_terminating() {
            return Outcome::Done;
        }

        match sts.spec.pod_management_policy {
            PodManagementPolicy::OrderedReady => reconcile_ordered(&sts, cluster),
            PodManagementPolicy::Parallel => reconcile_parallel(&sts, cluster),
        }

        write_status(&sts, cluster);
        Outcome::Done
    }
}

/// `OrderedReady`: at most one pod changes per reconcile, in ordinal order.
fn reconcile_ordered(sts: &StatefulSet, cluster: &mut Cluster) {
    let owned = cluster.pods.list_owned_by(&sts.meta.uid);

    // Scale down: delete the highest ordinal >= desired, one per reconcile.
    if let Some(victim) = owned
        .iter()
        .filter_map(|p| ordinal(&sts.meta.name, &p.meta.name).map(|o| (o, p)))
        .filter(|(o, _)| *o >= sts.spec.replicas)
        .max_by_key(|(o, _)| *o)
    {
        cluster.pods.delete(&victim.1.key()); // PVC is intentionally retained.
        return;
    }

    // Scale up: create the lowest missing ordinal, gated on predecessors being
    // ready (OrderedReady).
    for target in 0..sts.spec.replicas {
        let name = format!("{}-{target}", sts.meta.name);
        if owned.iter().any(|p| p.meta.name == name) {
            continue;
        }
        let predecessors_ready = (0..target).all(|o| {
            let pname = format!("{}-{o}", sts.meta.name);
            owned.iter().find(|p| p.meta.name == pname).is_some_and(|p| p.status.ready)
        });
        if predecessors_ready {
            ensure_pvcs(sts, target, cluster);
            create_pod(sts, target, cluster);
        }
        break; // one ordinal per reconcile
    }
}

/// `Parallel`: bring the whole ordinal set to the desired count in one pass.
fn reconcile_parallel(sts: &StatefulSet, cluster: &mut Cluster) {
    let owned = cluster.pods.list_owned_by(&sts.meta.uid);

    // Delete every surplus ordinal (>= desired); PVCs are retained.
    for pod in &owned {
        if let Some(o) = ordinal(&sts.meta.name, &pod.meta.name) {
            if o >= sts.spec.replicas {
                cluster.pods.delete(&pod.key());
            }
        }
    }
    // Create every missing ordinal below desired.
    for target in 0..sts.spec.replicas {
        let name = format!("{}-{target}", sts.meta.name);
        if owned.iter().any(|p| p.meta.name == name) {
            continue;
        }
        ensure_pvcs(sts, target, cluster);
        create_pod(sts, target, cluster);
    }
}

/// Parse the ordinal suffix from `<sts>-<ordinal>`; `None` if it does not match.
fn ordinal(sts_name: &str, pod_name: &str) -> Option<i32> {
    let prefix = format!("{sts_name}-");
    pod_name.strip_prefix(&prefix).and_then(|s| s.parse().ok())
}

/// Ensure a PVC exists for every `volumeClaimTemplate` at `ordinal`, named
/// `<template>-<sts>-<ordinal>`. Idempotent: existing claims are left alone (so
/// a recreated pod re-binds the same storage).
fn ensure_pvcs(sts: &StatefulSet, ordinal: i32, cluster: &mut Cluster) {
    for template in &sts.spec.volume_claim_templates {
        let name = format!("{template}-{}-{ordinal}", sts.meta.name);
        let key = pvc_key(&sts.meta.namespace, &name);
        if cluster.pvcs.get(&key).is_some() {
            continue;
        }
        let mut meta = ObjectMeta::new(&name, &sts.meta.namespace, "");
        meta.owner_references = vec![OwnerReference::to("StatefulSet", &sts.meta.name, &sts.meta.uid)
            .controller()];
        cluster.pvcs.create(PersistentVolumeClaim::new(meta));
    }
}

/// The store key for a namespaced PVC name.
fn pvc_key(namespace: &str, name: &str) -> String {
    if namespace.is_empty() {
        name.to_owned()
    } else {
        format!("{namespace}/{name}")
    }
}

/// Create the pod for `ordinal`, stamped with the template + controller owner.
fn create_pod(sts: &StatefulSet, ordinal: i32, cluster: &mut Cluster) {
    let name = format!("{}-{ordinal}", sts.meta.name);
    let mut meta = ObjectMeta::new(&name, &sts.meta.namespace, "");
    meta.labels = sts.spec.template.labels.clone();
    meta.owner_references = vec![OwnerReference::to("StatefulSet", &sts.meta.name, &sts.meta.uid)
        .controller()
        .blocking()];
    cluster.pods.create(Pod::new(meta));
}

/// Recompute and persist the `StatefulSet` status from its owned pods. Rolling
/// updates are deferred, so every pod is on the current == updated revision.
fn write_status(sts: &StatefulSet, cluster: &mut Cluster) {
    let owned = cluster.pods.list_owned_by(&sts.meta.uid);
    let replicas = clamp_i32(owned.len());
    let ready = clamp_i32(owned.iter().filter(|p| p.status.ready).count());
    if let Some(mut current) = cluster.statefulsets.get(&sts.key()) {
        current.status.replicas = replicas;
        current.status.ready_replicas = ready;
        current.status.current_replicas = replicas;
        current.status.updated_replicas = replicas;
        cluster.statefulsets.update(current);
    }
}

/// Saturating `usize -> i32` for replica counts.
fn clamp_i32(n: usize) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}
