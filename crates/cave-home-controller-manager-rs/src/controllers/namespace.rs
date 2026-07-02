// SPDX-License-Identifier: Apache-2.0
//! Namespace controller — drives a terminating namespace to empty, then drops
//! the `kubernetes` finalizer.
//!
//! Behavioural reimplementation of the documented `pkg/controller/namespace`
//! contract, reconciling against the in-memory apiserver. It enumerates the
//! namespace's content across every namespaced kind, applies the pure
//! [`namespace_sweep`](crate::controllers::cleanup::namespace_sweep) decision,
//! and then either deletes the remaining content or removes the finalizer
//! (which lets the apiserver delete the namespace object itself).

use crate::apis::Cluster;
use crate::controllers::cleanup::{namespace_sweep, NamespaceSweep, NAMESPACE_FINALIZER};
use crate::reconcile::Outcome;
use crate::types::Object;

/// The Namespace controller.
#[derive(Debug, Default)]
pub struct NamespaceController;

impl NamespaceController {
    /// A fresh controller.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Reconcile one namespace key (`"<name>"`).
    pub fn reconcile(&mut self, namespace: &str, cluster: &mut Cluster, _now: u64) -> Outcome {
        let Some(ns) = cluster.namespaces.get(namespace) else {
            return Outcome::Done;
        };

        let contents = namespaced_keys(cluster, namespace);
        match namespace_sweep(&ns.meta, &contents) {
            NamespaceSweep::NotTerminating | NamespaceSweep::AlreadyFinalized => {}
            NamespaceSweep::AwaitingContent { .. } => {
                delete_namespaced(cluster, namespace);
            }
            NamespaceSweep::RemoveFinalizer => {
                if let Some(mut current) = cluster.namespaces.get(namespace) {
                    current.meta.finalizers.retain(|f| f != NAMESPACE_FINALIZER);
                    cluster.namespaces.update(current);
                }
            }
        }
        Outcome::Done
    }
}

/// Every namespaced object's key living in `namespace`, across all kinds.
fn namespaced_keys(cluster: &Cluster, namespace: &str) -> Vec<String> {
    let mut keys = Vec::new();
    keys.extend(cluster.pods.list_namespaced(namespace).iter().map(Object::key));
    keys.extend(cluster.replicasets.list_namespaced(namespace).iter().map(Object::key));
    keys.extend(cluster.deployments.list_namespaced(namespace).iter().map(Object::key));
    keys.extend(cluster.statefulsets.list_namespaced(namespace).iter().map(Object::key));
    keys.extend(cluster.daemonsets.list_namespaced(namespace).iter().map(Object::key));
    keys.extend(cluster.jobs.list_namespaced(namespace).iter().map(Object::key));
    keys.extend(cluster.cronjobs.list_namespaced(namespace).iter().map(Object::key));
    keys.extend(cluster.endpoints.list_namespaced(namespace).iter().map(Object::key));
    keys.extend(cluster.services.list_namespaced(namespace).iter().map(Object::key));
    keys.extend(cluster.service_accounts.list_namespaced(namespace).iter().map(Object::key));
    keys
}

/// Delete every namespaced object in `namespace`, across all kinds.
fn delete_namespaced(cluster: &mut Cluster, namespace: &str) {
    for k in cluster.pods.list_namespaced(namespace).iter().map(Object::key) {
        cluster.pods.delete(&k);
    }
    for k in cluster.replicasets.list_namespaced(namespace).iter().map(Object::key) {
        cluster.replicasets.delete(&k);
    }
    for k in cluster.deployments.list_namespaced(namespace).iter().map(Object::key) {
        cluster.deployments.delete(&k);
    }
    for k in cluster.statefulsets.list_namespaced(namespace).iter().map(Object::key) {
        cluster.statefulsets.delete(&k);
    }
    for k in cluster.daemonsets.list_namespaced(namespace).iter().map(Object::key) {
        cluster.daemonsets.delete(&k);
    }
    for k in cluster.jobs.list_namespaced(namespace).iter().map(Object::key) {
        cluster.jobs.delete(&k);
    }
    for k in cluster.cronjobs.list_namespaced(namespace).iter().map(Object::key) {
        cluster.cronjobs.delete(&k);
    }
    for k in cluster.endpoints.list_namespaced(namespace).iter().map(Object::key) {
        cluster.endpoints.delete(&k);
    }
    for k in cluster.services.list_namespaced(namespace).iter().map(Object::key) {
        cluster.services.delete(&k);
    }
    for k in cluster.service_accounts.list_namespaced(namespace).iter().map(Object::key) {
        cluster.service_accounts.delete(&k);
    }
}
