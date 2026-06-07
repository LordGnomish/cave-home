// SPDX-License-Identifier: Apache-2.0
//! Deployment controller — manages `ReplicaSet`s to roll out a pod template.
//!
//! Behavioural reimplementation of the documented `pkg/controller/deployment`
//! contract, reconciling against the in-memory apiserver:
//!
//! * compute the `pod-template-hash` of the current template and find the
//!   **new** `ReplicaSet` (one whose hash matches) among the deployment's owned
//!   RSes; create it if absent (selector + template stamped with the hash, a
//!   controller + `blockOwnerDeletion` owner reference to the deployment);
//! * **`Recreate`** strategy: scale every old RS to 0 first, then bring the new
//!   RS up to `spec.replicas` once the old pods have drained;
//! * **`RollingUpdate`** strategy: surge the new RS up within
//!   `desired + maxSurge`, and scale old RSes down while keeping at least
//!   `desired - maxUnavailable` available (`NewRSNewReplicas` /
//!   `reconcileOldReplicaSets` math);
//! * aggregate owned-RS status into `status.{replicas,ready,available}`.
//!
//! Status lags reality (an RS reports availability only after its pods become
//! ready), so a rollout converges over several reconciles — exactly the
//! upstream behaviour, driven here by the run loop calling `reconcile` again.

use crate::apis::{template_hash, Cluster, Deployment, ReplicaSet, ReplicaSetSpec};
use crate::reconcile::Outcome;
use crate::types::{Object, ObjectMeta, OwnerReference};

/// The label key carrying a `ReplicaSet`'s pod-template hash (apps `apps/v1`
/// `pod-template-hash`), used to distinguish the new revision from old ones.
pub const POD_TEMPLATE_HASH: &str = "pod-template-hash";

/// The Deployment controller.
#[derive(Debug, Default)]
pub struct DeploymentController;

impl DeploymentController {
    /// A fresh controller.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Reconcile one Deployment key (`"<ns>/<name>"`).
    pub fn reconcile(&mut self, key: &str, cluster: &mut Cluster, _now: u64) -> Outcome {
        let Some(deploy) = cluster.deployments.get(key) else {
            return Outcome::Done;
        };
        if deploy.meta.is_terminating() {
            return Outcome::Done;
        }

        let hash = template_hash(&deploy.spec.template);
        let owned = cluster.replicasets.list_owned_by(&deploy.meta.uid);
        let new_rs = owned
            .iter()
            .find(|rs| rs.meta.labels.get(POD_TEMPLATE_HASH) == Some(&hash))
            .cloned()
            .unwrap_or_else(|| create_new_rs(&deploy, &hash, cluster));
        let old_rs: Vec<ReplicaSet> = owned
            .into_iter()
            .filter(|rs| rs.meta.labels.get(POD_TEMPLATE_HASH) != Some(&hash))
            .collect();

        match deploy.spec.strategy {
            crate::apis::DeploymentStrategy::Recreate => {
                reconcile_recreate(&deploy, &new_rs, &old_rs, cluster);
            }
            crate::apis::DeploymentStrategy::RollingUpdate { max_unavailable, max_surge } => {
                reconcile_rolling(&deploy, &new_rs, &old_rs, max_unavailable, max_surge, cluster);
            }
        }

        write_status(&deploy, cluster);
        Outcome::Done
    }
}

/// Create the new `ReplicaSet` for the current template, scaled to 0 (the
/// strategy step then scales it up). Selector and template are stamped with the
/// `pod-template-hash` so it owns only its own pods.
fn create_new_rs(deploy: &Deployment, hash: &str, cluster: &mut Cluster) -> ReplicaSet {
    let mut selector = deploy.spec.selector.clone();
    selector.insert(POD_TEMPLATE_HASH.to_owned(), hash.to_owned());
    let mut template = deploy.spec.template.clone();
    template.labels.insert(POD_TEMPLATE_HASH.to_owned(), hash.to_owned());

    let mut meta = ObjectMeta::new(&format!("{}-{hash}", deploy.meta.name), &deploy.meta.namespace, "");
    meta.labels = template.labels.clone();
    meta.owner_references = vec![OwnerReference::to(
        "Deployment",
        &deploy.meta.name,
        &deploy.meta.uid,
    )
    .controller()
    .blocking()];

    cluster
        .replicasets
        .create(ReplicaSet::new(meta, ReplicaSetSpec { replicas: 0, selector, template }))
}

/// `Recreate`: scale all old RSes to 0, hold the new RS at 0 until the old pods
/// have drained, then bring the new RS to `desired`.
fn reconcile_recreate(
    deploy: &Deployment,
    new_rs: &ReplicaSet,
    old_rs: &[ReplicaSet],
    cluster: &mut Cluster,
) {
    let old_present: i32 = old_rs.iter().map(|rs| rs.status.replicas).sum();
    if old_present > 0 {
        for rs in old_rs {
            scale(rs, 0, cluster);
        }
        scale(new_rs, 0, cluster);
    } else {
        scale(new_rs, deploy.spec.replicas, cluster);
    }
}

/// `RollingUpdate`: surge the new RS up within `desired + maxSurge`, then scale
/// old RSes down while keeping `desired - maxUnavailable` available.
fn reconcile_rolling(
    deploy: &Deployment,
    new_rs: &ReplicaSet,
    old_rs: &[ReplicaSet],
    max_unavailable: i32,
    max_surge: i32,
    cluster: &mut Cluster,
) {
    let desired = deploy.spec.replicas;

    // No rollout in progress (all old RSes drained): the new RS owns the whole
    // Deployment, so it tracks `desired` directly — this is the pure
    // scaling-event path (scale up *or* down), separate from the surge/cut
    // rollout math below (upstream `dc.scale`).
    let old_total: i32 = old_rs.iter().map(|rs| rs.spec.replicas).sum();
    if old_total == 0 {
        scale(new_rs, desired, cluster);
        return;
    }

    // --- scale up new (NewRSNewReplicas) ---
    let total_spec: i32 = old_rs.iter().map(|rs| rs.spec.replicas).sum::<i32>() + new_rs.spec.replicas;
    let max_total = desired + max_surge;
    let mut new_replicas = new_rs.spec.replicas;
    if total_spec < max_total {
        let scale_up = (max_total - total_spec).min(desired - new_rs.spec.replicas).max(0);
        new_replicas = new_rs.spec.replicas + scale_up;
        if new_replicas != new_rs.spec.replicas {
            scale(new_rs, new_replicas, cluster);
        }
    }

    // --- scale down old (reconcileOldReplicaSets) ---
    let available: i32 =
        old_rs.iter().map(|rs| rs.status.available_replicas).sum::<i32>() + new_rs.status.available_replicas;
    let min_available = (desired - max_unavailable).max(0);
    // Unavailable pods already committed to the new RS (its spec exceeds what's
    // observed available) must be reserved before cutting old capacity.
    let new_unavailable = (new_replicas - new_rs.status.available_replicas).max(0);
    let mut budget = available - min_available - new_unavailable;
    if budget > 0 {
        // Cut the largest old RSes first (upstream cleans up + scales the
        // remainder; "largest first" is the deterministic subset modelled here).
        let mut olds: Vec<ReplicaSet> = old_rs.to_vec();
        olds.sort_by_key(|rs| std::cmp::Reverse(rs.spec.replicas));
        for rs in &olds {
            if budget <= 0 {
                break;
            }
            let dec = rs.spec.replicas.min(budget);
            if dec > 0 {
                scale(rs, rs.spec.replicas - dec, cluster);
                budget -= dec;
            }
        }
    }
}

/// Set an RS's `spec.replicas`, persisting only if it changed.
fn scale(rs: &ReplicaSet, replicas: i32, cluster: &mut Cluster) {
    if rs.spec.replicas == replicas {
        return;
    }
    if let Some(mut current) = cluster.replicasets.get(&rs.key()) {
        current.spec.replicas = replicas;
        cluster.replicasets.update(current);
    }
}

/// Aggregate owned-RS observed status into the Deployment status.
fn write_status(deploy: &Deployment, cluster: &mut Cluster) {
    let owned = cluster.replicasets.list_owned_by(&deploy.meta.uid);
    let hash = template_hash(&deploy.spec.template);
    let replicas: i32 = owned.iter().map(|rs| rs.status.replicas).sum();
    let ready: i32 = owned.iter().map(|rs| rs.status.ready_replicas).sum();
    let available: i32 = owned.iter().map(|rs| rs.status.available_replicas).sum();
    let updated: i32 = owned
        .iter()
        .filter(|rs| rs.meta.labels.get(POD_TEMPLATE_HASH) == Some(&hash))
        .map(|rs| rs.status.replicas)
        .sum();
    if let Some(mut current) = cluster.deployments.get(&deploy.key()) {
        current.status.replicas = replicas;
        current.status.updated_replicas = updated;
        current.status.ready_replicas = ready;
        current.status.available_replicas = available;
        cluster.deployments.update(current);
    }
}
