// SPDX-License-Identifier: Apache-2.0
//! `DaemonSet` controller — one pod per node.
//!
//! Behavioural reimplementation of the documented `pkg/controller/daemonset`
//! contract, reconciling against the in-memory apiserver:
//!
//! * for every node with no daemon pod, create one (named `<ds>-<node>`,
//!   carrying the template labels, a controller + `blockOwnerDeletion` owner
//!   reference, and a [`DS_NODE_LABEL`] recording its node);
//! * delete daemon pods whose node no longer exists.
//!
//! Node-selector/taint-toleration filtering of *which* nodes are eligible is the
//! scheduler's concern; this models the all-nodes default placement.

use crate::apis::{Cluster, Pod};
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

        let node_names: Vec<String> = cluster.nodes.list().iter().map(|n| n.meta.name.clone()).collect();
        let pods = cluster.pods.list_owned_by(&ds.meta.uid);

        // Delete daemon pods whose node is gone.
        for pod in &pods {
            let on_node = pod.meta.labels.get(DS_NODE_LABEL);
            if on_node.is_none_or(|n| !node_names.contains(n)) {
                cluster.pods.delete(&pod.key());
            }
        }

        // Create a pod on every node that lacks one.
        let covered: Vec<String> = pods
            .iter()
            .filter_map(|p| p.meta.labels.get(DS_NODE_LABEL).cloned())
            .filter(|n| node_names.contains(n))
            .collect();
        for node in &node_names {
            if !covered.contains(node) {
                create_pod(&ds.meta.name, &ds.meta.namespace, &ds.meta.uid, node, &ds.spec.template.labels, cluster);
            }
        }

        Outcome::Done
    }
}

/// Create the daemon pod for `node`.
fn create_pod(
    ds_name: &str,
    namespace: &str,
    ds_uid: &str,
    node: &str,
    template_labels: &std::collections::BTreeMap<String, String>,
    cluster: &mut Cluster,
) {
    let mut meta = ObjectMeta::new(&format!("{ds_name}-{node}"), namespace, "");
    meta.labels = template_labels.clone();
    meta.labels.insert(DS_NODE_LABEL.to_owned(), node.to_owned());
    meta.owner_references = vec![OwnerReference::to("DaemonSet", ds_name, ds_uid)
        .controller()
        .blocking()];
    cluster.pods.create(Pod::new(meta));
}
