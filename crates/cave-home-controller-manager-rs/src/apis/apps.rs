// SPDX-License-Identifier: Apache-2.0
//! `apps/v1` object subset: [`ReplicaSet`], [`Deployment`], [`StatefulSet`]
//! and [`DaemonSet`] — spec (desired) + status (observed).

use crate::apis::{LabelSelector, PodTemplateSpec};
use crate::types::{Object, ObjectMeta};

/// Desired state of a `ReplicaSet` (`apps/v1` `ReplicaSetSpec` subset).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReplicaSetSpec {
    /// Desired number of active pods.
    pub replicas: i32,
    /// Label selector identifying the pods this RS owns.
    pub selector: LabelSelector,
    /// Template stamped out for new pods.
    pub template: PodTemplateSpec,
}

/// Observed state of a `ReplicaSet` (`apps/v1` `ReplicaSetStatus` subset).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReplicaSetStatus {
    /// Most recently observed number of active pods.
    pub replicas: i32,
    /// Number of ready pods.
    pub ready_replicas: i32,
    /// Number of available pods (ready; here we treat ready==available).
    pub available_replicas: i32,
}

/// A `ReplicaSet` (`apps/v1` `ReplicaSet` subset).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReplicaSet {
    /// Object metadata.
    pub meta: ObjectMeta,
    /// Desired state.
    pub spec: ReplicaSetSpec,
    /// Observed state.
    pub status: ReplicaSetStatus,
}

impl ReplicaSet {
    /// A `ReplicaSet` with the given metadata and spec, empty status.
    #[must_use]
    pub fn new(meta: ObjectMeta, spec: ReplicaSetSpec) -> Self {
        Self { meta, spec, status: ReplicaSetStatus::default() }
    }
}

impl Object for ReplicaSet {
    fn meta(&self) -> &ObjectMeta {
        &self.meta
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.meta
    }
}

/// A Deployment's rollout strategy (`apps/v1` `DeploymentStrategy` subset).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentStrategy {
    /// Kill all old pods before creating new ones.
    Recreate,
    /// Gradually replace old pods, bounded by surge/unavailable.
    RollingUpdate {
        /// Max pods that may be unavailable during the rollout.
        max_unavailable: i32,
        /// Max pods that may exist above desired during the rollout.
        max_surge: i32,
    },
}

impl Default for DeploymentStrategy {
    /// Upstream default: 25% surge / 25% unavailable — modelled here as 1/1,
    /// the small-replica rounding the home cluster sees.
    fn default() -> Self {
        Self::RollingUpdate { max_unavailable: 1, max_surge: 1 }
    }
}

/// Desired state of a Deployment (`apps/v1` `DeploymentSpec` subset).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeploymentSpec {
    /// Desired number of pods.
    pub replicas: i32,
    /// Selector for pods/ReplicaSets this Deployment owns.
    pub selector: LabelSelector,
    /// Template for the current revision.
    pub template: PodTemplateSpec,
    /// Rollout strategy.
    pub strategy: DeploymentStrategy,
}

/// Observed state of a Deployment (`apps/v1` `DeploymentStatus` subset).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeploymentStatus {
    /// Total non-terminated pods across all owned `ReplicaSets`.
    pub replicas: i32,
    /// Pods on the current (updated) template.
    pub updated_replicas: i32,
    /// Ready pods.
    pub ready_replicas: i32,
    /// Available pods.
    pub available_replicas: i32,
}

/// A Deployment (`apps/v1` `Deployment` subset).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Deployment {
    /// Object metadata.
    pub meta: ObjectMeta,
    /// Desired state.
    pub spec: DeploymentSpec,
    /// Observed state.
    pub status: DeploymentStatus,
}

impl Deployment {
    /// A Deployment with the given metadata and spec, empty status.
    #[must_use]
    pub fn new(meta: ObjectMeta, spec: DeploymentSpec) -> Self {
        Self { meta, spec, status: DeploymentStatus::default() }
    }
}

impl Object for Deployment {
    fn meta(&self) -> &ObjectMeta {
        &self.meta
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.meta
    }
}

/// `apps/v1` `StatefulSetSpec` subset: ordered, stably-named replicas.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatefulSetSpec {
    /// Desired replicas (ordinals `0..replicas`).
    pub replicas: i32,
    /// Selector for owned pods.
    pub selector: LabelSelector,
    /// Pod template.
    pub template: PodTemplateSpec,
}

/// `apps/v1` `StatefulSet` subset.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatefulSet {
    /// Object metadata.
    pub meta: ObjectMeta,
    /// Desired state.
    pub spec: StatefulSetSpec,
}

impl StatefulSet {
    /// A `StatefulSet` with the given metadata and spec.
    #[must_use]
    pub const fn new(meta: ObjectMeta, spec: StatefulSetSpec) -> Self {
        Self { meta, spec }
    }
}

impl Object for StatefulSet {
    fn meta(&self) -> &ObjectMeta {
        &self.meta
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.meta
    }
}

/// `apps/v1` `DaemonSetSpec` subset: one pod per matching node.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DaemonSetSpec {
    /// Selector for owned pods.
    pub selector: LabelSelector,
    /// Pod template.
    pub template: PodTemplateSpec,
}

/// `apps/v1` `DaemonSet` subset.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DaemonSet {
    /// Object metadata.
    pub meta: ObjectMeta,
    /// Desired state.
    pub spec: DaemonSetSpec,
}

impl DaemonSet {
    /// A `DaemonSet` with the given metadata and spec.
    #[must_use]
    pub const fn new(meta: ObjectMeta, spec: DaemonSetSpec) -> Self {
        Self { meta, spec }
    }
}

impl Object for DaemonSet {
    fn meta(&self) -> &ObjectMeta {
        &self.meta
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.meta
    }
}
