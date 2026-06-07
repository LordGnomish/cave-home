// SPDX-License-Identifier: Apache-2.0
//! `core/v1` object subset: [`Pod`], [`Namespace`], [`Node`], [`Endpoints`]
//! and [`ServiceAccount`] — only the fields the controllers read.

use std::collections::BTreeMap;

use crate::types::{Object, ObjectMeta};

/// A pod's lifecycle phase (`core/v1` `PodPhase`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PodPhase {
    /// Accepted but not yet running.
    #[default]
    Pending,
    /// At least one container is running.
    Running,
    /// All containers terminated successfully.
    Succeeded,
    /// All containers terminated and at least one failed.
    Failed,
    /// State could not be obtained.
    Unknown,
}

impl PodPhase {
    /// `true` once the pod has reached a terminal phase (`Succeeded`/`Failed`).
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed)
    }
}

/// Observed pod state (`core/v1` `PodStatus` subset).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PodStatus {
    /// Lifecycle phase.
    pub phase: PodPhase,
    /// Whether the pod's `Ready` condition is true.
    pub ready: bool,
}

/// A pod (`core/v1` `Pod` subset).
///
/// Spec detail beyond labels is irrelevant to the controller decisions modelled
/// here, so the template carried by parents supplies the labels and the pod's
/// identity comes from its [`ObjectMeta`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pod {
    /// Object metadata.
    pub meta: ObjectMeta,
    /// Observed status.
    pub status: PodStatus,
}

impl Pod {
    /// A pending pod with the given metadata.
    #[must_use]
    pub fn new(meta: ObjectMeta) -> Self {
        Self { meta, status: PodStatus::default() }
    }

    /// `true` if the pod is active: not terminal and not terminating. This is
    /// the upstream notion of a pod that "counts" toward a controller's replica
    /// total (`controller.IsPodActive`).
    #[must_use]
    pub const fn is_active(&self) -> bool {
        !self.status.phase.is_terminal() && !self.meta.is_terminating()
    }
}

impl Object for Pod {
    fn meta(&self) -> &ObjectMeta {
        &self.meta
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.meta
    }
}

/// The pod template a workload controller stamps out (`core/v1`
/// `PodTemplateSpec` subset): the labels applied to every pod it creates.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PodTemplateSpec {
    /// Labels applied to pods created from this template.
    pub labels: BTreeMap<String, String>,
}

impl PodTemplateSpec {
    /// A template carrying the given labels.
    #[must_use]
    pub fn with_labels(pairs: &[(&str, &str)]) -> Self {
        let mut labels = BTreeMap::new();
        for (k, v) in pairs {
            labels.insert((*k).to_owned(), (*v).to_owned());
        }
        Self { labels }
    }
}

/// A namespace (`core/v1` `Namespace` subset). Termination is driven by
/// `meta.deletion_timestamp` + the `kubernetes` finalizer (see
/// `controllers::cleanup`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Namespace {
    /// Object metadata (cluster-scoped: empty namespace).
    pub meta: ObjectMeta,
}

impl Namespace {
    /// A namespace object with the given name.
    #[must_use]
    pub fn new(name: &str, uid: &str) -> Self {
        Self { meta: ObjectMeta::new(name, "", uid) }
    }
}

impl Object for Namespace {
    fn meta(&self) -> &ObjectMeta {
        &self.meta
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.meta
    }
}

/// A node's readiness condition (`core/v1` node `Ready` condition).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeCondition {
    /// kubelet reports the node healthy.
    Ready,
    /// kubelet reports the node unhealthy.
    NotReady,
    /// kubelet status unknown.
    Unknown,
}

/// A node (`core/v1` `Node` subset).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// Object metadata (cluster-scoped).
    pub meta: ObjectMeta,
    /// The node's `Ready` condition.
    pub condition: NodeCondition,
    /// Epoch-seconds of the last heartbeat.
    pub last_heartbeat: i64,
}

impl Node {
    /// A ready node reporting at `last_heartbeat`.
    #[must_use]
    pub fn new(name: &str, uid: &str, last_heartbeat: i64) -> Self {
        Self {
            meta: ObjectMeta::new(name, "", uid),
            condition: NodeCondition::Ready,
            last_heartbeat,
        }
    }
}

impl Object for Node {
    fn meta(&self) -> &ObjectMeta {
        &self.meta
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.meta
    }
}

/// An `Endpoints` object (`core/v1` `Endpoints` subset): the set of ready pod
/// IPs backing a service. Modelled as a sorted list of addresses.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Endpoints {
    /// Object metadata; its name matches the service it backs.
    pub meta: ObjectMeta,
    /// Ready backend addresses (here, pod keys), kept sorted.
    pub addresses: Vec<String>,
}

impl Endpoints {
    /// An empty endpoints object named after its service.
    #[must_use]
    pub const fn new(meta: ObjectMeta) -> Self {
        Self { meta, addresses: Vec::new() }
    }
}

impl Object for Endpoints {
    fn meta(&self) -> &ObjectMeta {
        &self.meta
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.meta
    }
}

/// A `Service` (`core/v1` `Service` subset): a name + pod selector. The
/// Endpoints controller turns its selector into a backing address set.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Service {
    /// Object metadata.
    pub meta: ObjectMeta,
    /// Selector identifying the pods that back this service.
    pub selector: BTreeMap<String, String>,
}

impl Service {
    /// A service with the given metadata and selector.
    #[must_use]
    pub fn new(meta: ObjectMeta, selector: BTreeMap<String, String>) -> Self {
        Self { meta, selector }
    }
}

impl Object for Service {
    fn meta(&self) -> &ObjectMeta {
        &self.meta
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.meta
    }
}

/// A `ServiceAccount` (`core/v1` `ServiceAccount` subset).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ServiceAccount {
    /// Object metadata.
    pub meta: ObjectMeta,
    /// Names of secrets mounted for this account (the default-token controller
    /// ensures one exists).
    pub secrets: Vec<String>,
}

impl ServiceAccount {
    /// A service account with the given metadata and no secrets.
    #[must_use]
    pub const fn new(meta: ObjectMeta) -> Self {
        Self { meta, secrets: Vec::new() }
    }
}

impl Object for ServiceAccount {
    fn meta(&self) -> &ObjectMeta {
        &self.meta
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.meta
    }
}
