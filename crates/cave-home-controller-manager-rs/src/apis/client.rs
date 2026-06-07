// SPDX-License-Identifier: Apache-2.0
//! The in-memory apiserver: [`Api`] (one resource kind) and [`Cluster`]
//! (a bundle of typed `Api`s the controllers share).
//!
//! [`Api`] is a faithful in-memory implementation of the create/get/update/
//! delete/list contract — the analogue of client-go's `testing.ObjectTracker`
//! that every upstream controller test runs against. It is **not** a stub: it
//! assigns UIDs, keeps the [`Store`] indices consistent, and answers
//! selector/owner queries exactly as the real server would for these fields.
//! Only the *network transport* (REST + watch) is deferred.

use crate::apis::{
    selector_matches, CronJob, DaemonSet, Deployment, Endpoints, Job, LabelSelector, Namespace,
    Node, Pod, ReplicaSet, ServiceAccount, StatefulSet,
};
use crate::informer::Store;
use crate::types::Object;

/// An in-memory collection of one resource kind, behind the create/get/update/
/// delete/list contract.
#[derive(Debug, Clone)]
pub struct Api<T: Object> {
    store: Store<T>,
    kind: String,
    seq: u64,
}

impl<T: Object> Api<T> {
    /// An empty `Api` for the named kind (the kind seeds generated UIDs).
    #[must_use]
    pub fn new(kind: &str) -> Self {
        Self { store: Store::new(), kind: kind.to_owned(), seq: 0 }
    }

    /// Create an object, assigning a server UID if it has none. Returns the
    /// stored object (with its UID populated), mirroring the apiserver's
    /// echo-back of the persisted resource.
    pub fn create(&mut self, mut obj: T) -> T {
        if obj.meta().uid.is_empty() {
            self.seq += 1;
            obj.meta_mut().uid = format!("{}-{:08}", self.kind, self.seq);
        }
        self.store.upsert(obj.clone());
        obj
    }

    /// Replace an existing object (apiserver `Update`). If the object is absent
    /// this inserts it, matching the fake tracker's lenient update.
    pub fn update(&mut self, obj: T) {
        self.store.upsert(obj);
    }

    /// Delete by key. Returns the removed object, if any.
    pub fn delete(&mut self, key: &str) -> Option<T> {
        self.store.remove(key)
    }

    /// Fetch by key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<T> {
        self.store.get(key)
    }

    /// Every object, key-sorted.
    #[must_use]
    pub fn list(&self) -> Vec<T> {
        self.store.list()
    }

    /// Objects in a namespace, key-sorted.
    #[must_use]
    pub fn list_namespaced(&self, namespace: &str) -> Vec<T> {
        self.store.list_by_namespace(namespace)
    }

    /// Objects in `namespace` matching `selector` (AND semantics). An empty
    /// selector matches nothing here (a controller must have a real selector);
    /// callers that want "everything" use [`Api::list_namespaced`].
    #[must_use]
    pub fn list_matching(&self, namespace: &str, selector: &LabelSelector) -> Vec<T> {
        if selector.is_empty() {
            return Vec::new();
        }
        self.store
            .list_by_namespace(namespace)
            .into_iter()
            .filter(|o| selector_matches(selector, &o.meta().labels))
            .collect()
    }

    /// Objects whose **controller** owner reference is `owner_uid`, key-sorted.
    /// This is the `controller.getPodsForController`-style query controllers use
    /// to find the children they manage.
    #[must_use]
    pub fn list_owned_by(&self, owner_uid: &str) -> Vec<T> {
        self.store
            .list()
            .into_iter()
            .filter(|o| {
                o.meta()
                    .owner_references
                    .iter()
                    .any(|r| r.controller && r.uid == owner_uid)
            })
            .collect()
    }

    /// Borrow the underlying [`Store`] — this is the informer cache view a
    /// shared-informer event handler reads.
    #[must_use]
    pub const fn store(&self) -> &Store<T> {
        &self.store
    }

    /// Number of objects held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.store.len()
    }

    /// `true` if the collection is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.store.is_empty()
    }
}

/// The in-memory apiserver: one [`Api`] per resource kind the controllers use.
///
/// A single `Cluster` is shared by every controller (and, in the e2e tests,
/// driven to convergence). This is the seam where a real run loop would
/// substitute a networked clientset.
#[derive(Debug, Clone)]
pub struct Cluster {
    /// Pods (`core/v1`).
    pub pods: Api<Pod>,
    /// `ReplicaSets` (`apps/v1`).
    pub replicasets: Api<ReplicaSet>,
    /// Deployments (`apps/v1`).
    pub deployments: Api<Deployment>,
    /// `StatefulSets` (`apps/v1`).
    pub statefulsets: Api<StatefulSet>,
    /// `DaemonSets` (`apps/v1`).
    pub daemonsets: Api<DaemonSet>,
    /// Jobs (`batch/v1`).
    pub jobs: Api<Job>,
    /// `CronJobs` (`batch/v1`).
    pub cronjobs: Api<CronJob>,
    /// Namespaces (`core/v1`).
    pub namespaces: Api<Namespace>,
    /// Nodes (`core/v1`).
    pub nodes: Api<Node>,
    /// Endpoints (`core/v1`).
    pub endpoints: Api<Endpoints>,
    /// `ServiceAccounts` (`core/v1`).
    pub service_accounts: Api<ServiceAccount>,
}

impl Default for Cluster {
    fn default() -> Self {
        Self::new()
    }
}

impl Cluster {
    /// An empty in-memory cluster.
    #[must_use]
    pub fn new() -> Self {
        Self {
            pods: Api::new("pod"),
            replicasets: Api::new("rs"),
            deployments: Api::new("deploy"),
            statefulsets: Api::new("sts"),
            daemonsets: Api::new("ds"),
            jobs: Api::new("job"),
            cronjobs: Api::new("cronjob"),
            namespaces: Api::new("ns"),
            nodes: Api::new("node"),
            endpoints: Api::new("ep"),
            service_accounts: Api::new("sa"),
        }
    }
}
