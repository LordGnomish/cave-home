// SPDX-License-Identifier: Apache-2.0
//! `DaemonSet` controller — one pod per *eligible* node.
//!
//! Behavioural reimplementation of the documented `pkg/controller/daemonset`
//! contract, reconciling against the in-memory apiserver:
//!
//! * a node is **eligible** when its labels satisfy the daemonset's
//!   `spec.template.spec.nodeSelector` (AND semantics; an empty selector means
//!   every node) — the placement predicate the daemon-set controller computes
//!   via `nodeShouldRunDaemonPod`;
//! * for every eligible node with no daemon pod, create one (named
//!   `<ds>-<node>`, carrying the template labels, a controller +
//!   `blockOwnerDeletion` owner reference, and a [`DS_NODE_LABEL`] recording its
//!   node);
//! * delete daemon pods whose node is gone **or no longer eligible**;
//! * write `status.{desired,current}_number_scheduled`, `number_ready` and
//!   `number_misscheduled`.
//!
//! Taint/toleration filtering is the scheduler's concern; this models the
//! nodeSelector predicate the controller itself applies.

use std::collections::BTreeMap;

use crate::apis::{selector_matches, Cluster, DaemonSet, Node, Pod};
use crate::reconcile::Outcome;
use crate::types::{Object, ObjectMeta, OwnerReference};

/// Label recording which node a daemon pod was placed on.
pub const DS_NODE_LABEL: &str = "controller.cave-home/daemon-node";

/// The `DaemonSet` controller.
#[derive(Debug, Default)]
pub struct DaemonSetController;

impl DaemonSetController {
    /// A fresh controller.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Reconcile one `DaemonSet` key (`"<ns>/<name>"`).
    pub fn reconcile(&mut self, key: &str, cluster: &mut Cluster, _now: u64) -> Outcome {
        let Some(ds) = cluster.daemonsets.get(key) else {
            return Outcome::Done;
        };
        if ds.meta.is_terminating() {
            return Outcome::Done;
        }

        // Eligible nodes: those whose labels satisfy the nodeSelector.
        let eligible: Vec<String> = cluster
            .nodes
            .list()
            .iter()
            .filter(|n| node_eligible(n, &ds.spec.node_selector))
            .map(|n| n.meta.name.clone())
            .collect();

        let pods = cluster.pods.list_owned_by(&ds.meta.uid);

        // Delete daemon pods whose node is gone or no longer eligible.
        for pod in &pods {
            let on_node = pod.meta.labels.get(DS_NODE_LABEL);
            if on_node.is_none_or(|n| !eligible.contains(n)) {
                cluster.pods.delete(&pod.key());
            }
        }

        // Create a pod on every eligible node that lacks one.
        let covered: Vec<String> = pods
            .iter()
            .filter_map(|p| p.meta.labels.get(DS_NODE_LABEL).cloned())
            .filter(|n| eligible.contains(n))
            .collect();
        for node in &eligible {
            if !covered.contains(node) {
                create_pod(&ds, node, cluster);
            }
        }

        write_status(&ds, &eligible, cluster);
        Outcome::Done
    }
}

/// `true` if `node`'s labels satisfy the daemonset's nodeSelector. An empty
/// selector matches every node (the all-nodes default placement).
fn node_eligible(node: &Node, node_selector: &BTreeMap<String, String>) -> bool {
    node_selector.is_empty() || selector_matches(node_selector, &node.meta.labels)
}

/// Create the daemon pod for `node`.
fn create_pod(ds: &DaemonSet, node: &str, cluster: &mut Cluster) {
    let mut meta = ObjectMeta::new(&format!("{}-{node}", ds.meta.name), &ds.meta.namespace, "");
    meta.labels = ds.spec.template.labels.clone();
    meta.labels.insert(DS_NODE_LABEL.to_owned(), node.to_owned());
    meta.owner_references = vec![OwnerReference::to("DaemonSet", &ds.meta.name, &ds.meta.uid)
        .controller()
        .blocking()];
    cluster.pods.create(Pod::new(meta));
}

/// Recompute and persist the `DaemonSet` status from the current pods and the
/// eligible-node set.
fn write_status(ds: &DaemonSet, eligible: &[String], cluster: &mut Cluster) {
    let pods = cluster.pods.list_owned_by(&ds.meta.uid);
    let current = pods
        .iter()
        .filter(|p| p.meta.labels.get(DS_NODE_LABEL).is_some_and(|n| eligible.contains(n)))
        .count();
    let ready = pods
        .iter()
        .filter(|p| {
            p.status.ready
                && p.meta.labels.get(DS_NODE_LABEL).is_some_and(|n| eligible.contains(n))
        })
        .count();
    // A pod on a node that is no longer eligible is misscheduled (it is deleted
    // earlier in the same reconcile, so in steady state this is zero; it is
    // non-zero only transiently within a pass).
    let misscheduled = pods
        .iter()
        .filter(|p| p.meta.labels.get(DS_NODE_LABEL).is_some_and(|n| !eligible.contains(n)))
        .count();

    if let Some(mut updated) = cluster.daemonsets.get(&ds.key()) {
        updated.status.desired_number_scheduled = clamp_i32(eligible.len());
        updated.status.current_number_scheduled = clamp_i32(current);
        updated.status.number_ready = clamp_i32(ready);
        updated.status.number_misscheduled = clamp_i32(misscheduled);
        cluster.daemonsets.update(updated);
    }
}

/// Saturating `usize -> i32` for counts.
fn clamp_i32(n: usize) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}
