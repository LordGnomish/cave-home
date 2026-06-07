// SPDX-License-Identifier: Apache-2.0
//! `StatefulSet` controller — ordered, stably-named replicas.
//!
//! Behavioural reimplementation of the documented `pkg/controller/statefulset`
//! `OrderedReady` contract, reconciling against the in-memory apiserver:
//!
//! * pods are named `<sts>-<ordinal>` for ordinals `0..spec.replicas`;
//! * **scale up** creates the lowest missing ordinal, but only once every
//!   lower-ordinal pod is `Running` + ready (one step per reconcile);
//! * **scale down** deletes the highest ordinal at or above `spec.replicas`,
//!   one per reconcile, highest first.
//!
//! This preserves the at-most-one-changing, ordered-identity guarantee that
//! distinguishes a `StatefulSet` from a `ReplicaSet`.

use crate::apis::{Cluster, Pod, StatefulSet};
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

        let owned = cluster.pods.list_owned_by(&sts.meta.uid);

        // Scale down: delete the highest ordinal >= desired, one per reconcile.
        if let Some(victim) = owned
            .iter()
            .filter_map(|p| ordinal(&sts.meta.name, &p.meta.name).map(|o| (o, p)))
            .filter(|(o, _)| *o >= sts.spec.replicas)
            .max_by_key(|(o, _)| *o)
        {
            cluster.pods.delete(&victim.1.key());
            return Outcome::Done;
        }

        // Scale up: create the lowest missing ordinal, gated on predecessors
        // being ready (OrderedReady).
        for target in 0..sts.spec.replicas {
            let name = format!("{}-{target}", sts.meta.name);
            let exists = owned.iter().any(|p| p.meta.name == name);
            if exists {
                continue;
            }
            // All lower ordinals must exist and be ready before creating this.
            let predecessors_ready = (0..target).all(|o| {
                let pname = format!("{}-{o}", sts.meta.name);
                owned
                    .iter()
                    .find(|p| p.meta.name == pname)
                    .is_some_and(|p| p.status.ready)
            });
            if predecessors_ready {
                create_pod(&sts, target, cluster);
            }
            break; // one ordinal per reconcile
        }

        Outcome::Done
    }
}

/// Parse the ordinal suffix from `<sts>-<ordinal>`; `None` if it does not match.
fn ordinal(sts_name: &str, pod_name: &str) -> Option<i32> {
    let prefix = format!("{sts_name}-");
    pod_name.strip_prefix(&prefix).and_then(|s| s.parse().ok())
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
