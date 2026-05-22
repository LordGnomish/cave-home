// SPDX-License-Identifier: Apache-2.0
// Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//         staging/src/k8s.io/api/{apps,batch,core}/v1/types.go
//
//! Kubernetes API type subset consumed by the Phase 2 controllers.
//!
//! Phase 2 deliberately stays away from `k8s-openapi` / `kube-rs`; only the
//! fields the controllers actually read or write are modelled. Anything not
//! present here is recorded in `parity.manifest.toml` as `[[unmapped]]`.

use std::collections::BTreeMap;

/// Opaque resource UID (`k8s.io/apimachinery/pkg/types.UID`).
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Uid(pub String);

impl Uid {
    pub fn new<S: Into<String>>(uid: S) -> Self {
        Self(uid.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// `metav1.OwnerReference`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OwnerReference {
    pub api_version: String,
    pub kind: String,
    pub name: String,
    pub uid: Uid,
    /// Whether this owner manages the dependent (`metav1.OwnerReference.Controller`).
    pub controller: bool,
    /// `BlockOwnerDeletion` — if true, deletion of the owner is blocked until
    /// the dependent's finalizers run.
    pub block_owner_deletion: bool,
}

/// `metav1.LabelSelector` — Phase 2 supports `matchLabels` only. `matchExpressions`
/// is recorded as `[[unmapped]]`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LabelSelector {
    pub match_labels: BTreeMap<String, String>,
}

impl LabelSelector {
    /// Returns true if every label in `match_labels` is present and equal in
    /// `labels`.
    #[must_use]
    pub fn matches(&self, labels: &BTreeMap<String, String>) -> bool {
        self.match_labels
            .iter()
            .all(|(k, v)| labels.get(k).map_or(false, |actual| actual == v))
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.match_labels.is_empty()
    }
}

/// `metav1.ObjectMeta` subset.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ObjectMeta {
    pub name: String,
    pub namespace: String,
    pub uid: Uid,
    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,
    pub owner_references: Vec<OwnerReference>,
    pub finalizers: Vec<String>,
    /// `DeletionTimestamp` as unix-millis; `None` means object is not being
    /// deleted.
    pub deletion_timestamp_ms: Option<u64>,
    /// Monotonic resource version (controllers compare for stale writes).
    pub resource_version: u64,
    /// Monotonic generation (incremented when `spec` changes).
    pub generation: u64,
}

/// A namespaced or cluster-scoped k8s API object as seen by a controller.
///
/// Mirrors the `metav1.Object` interface (the small subset every controller
/// actually consults) plus a `kind()` so the [`crate::api_client::ControllerApiClient`]
/// can route generic CRUD without trait-object juggling.
pub trait KubeResource: Clone + Send + Sync + 'static {
    /// `metav1.TypeMeta.Kind`.
    fn kind() -> &'static str;

    fn meta(&self) -> &ObjectMeta;
    fn meta_mut(&mut self) -> &mut ObjectMeta;

    fn name(&self) -> &str {
        &self.meta().name
    }
    fn namespace(&self) -> &str {
        &self.meta().namespace
    }
    fn uid(&self) -> &Uid {
        &self.meta().uid
    }
    fn labels(&self) -> &BTreeMap<String, String> {
        &self.meta().labels
    }
}

// ---------------------------------------------------------------------------
// core/v1 — Pod, Container, Volume
// ---------------------------------------------------------------------------

/// `core/v1.RestartPolicy`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RestartPolicy {
    #[default]
    Always,
    OnFailure,
    Never,
}

/// `core/v1.Container` subset.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Container {
    pub name: String,
    pub image: String,
    pub command: Vec<String>,
    pub args: Vec<String>,
}

/// `core/v1.PodTemplateSpec`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PodTemplateSpec {
    pub metadata: ObjectMeta,
    pub spec: PodSpec,
}

/// `core/v1.PodSpec` subset.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PodSpec {
    pub containers: Vec<Container>,
    pub restart_policy: RestartPolicy,
    pub node_name: String,
    /// Phase 2 stores a single named-PVC volume mapping per pod (used by
    /// StatefulSet). `[[unmapped]]` covers the rest.
    pub volume_claims: Vec<String>,
}

/// `core/v1.PodPhase`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PodPhase {
    #[default]
    Pending,
    Running,
    Succeeded,
    Failed,
    Unknown,
}

/// `core/v1.PodStatus` subset.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PodStatus {
    pub phase: PodPhase,
    pub message: String,
    pub reason: String,
}

/// `core/v1.Pod`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Pod {
    pub metadata: ObjectMeta,
    pub spec: PodSpec,
    pub status: PodStatus,
}

impl KubeResource for Pod {
    fn kind() -> &'static str {
        "Pod"
    }
    fn meta(&self) -> &ObjectMeta {
        &self.metadata
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.metadata
    }
}

// ---------------------------------------------------------------------------
// apps/v1 — Deployment, ReplicaSet, DaemonSet, StatefulSet
// ---------------------------------------------------------------------------

/// `apps/v1.DeploymentStrategyType`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DeploymentStrategyType {
    Recreate,
    #[default]
    RollingUpdate,
}

/// `apps/v1.RollingUpdateDeployment` — Phase 2 honours absolute integers only
/// (the percentage form is `[[unmapped]]`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RollingUpdateDeployment {
    pub max_unavailable: i32,
    pub max_surge: i32,
}

impl Default for RollingUpdateDeployment {
    fn default() -> Self {
        // Upstream defaults — 25 % each, rounded to integers when replicas <= 4.
        // Phase 2 picks 1/1 which mirrors the small-cluster behaviour the
        // controllers see.
        Self {
            max_unavailable: 1,
            max_surge: 1,
        }
    }
}

/// `apps/v1.DeploymentStrategy`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DeploymentStrategy {
    pub kind: DeploymentStrategyType,
    pub rolling_update: RollingUpdateDeployment,
}

/// `apps/v1.DeploymentSpec`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DeploymentSpec {
    pub replicas: i32,
    pub selector: LabelSelector,
    pub template: PodTemplateSpec,
    pub strategy: DeploymentStrategy,
    /// `revisionHistoryLimit` (default 10 upstream).
    pub revision_history_limit: i32,
}

/// `apps/v1.DeploymentStatus`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DeploymentStatus {
    pub observed_generation: u64,
    pub replicas: i32,
    pub updated_replicas: i32,
    pub ready_replicas: i32,
    pub available_replicas: i32,
    pub unavailable_replicas: i32,
}

/// `apps/v1.Deployment`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Deployment {
    pub metadata: ObjectMeta,
    pub spec: DeploymentSpec,
    pub status: DeploymentStatus,
}

impl KubeResource for Deployment {
    fn kind() -> &'static str {
        "Deployment"
    }
    fn meta(&self) -> &ObjectMeta {
        &self.metadata
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.metadata
    }
}

/// `apps/v1.ReplicaSetSpec`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReplicaSetSpec {
    pub replicas: i32,
    pub selector: LabelSelector,
    pub template: PodTemplateSpec,
}

/// `apps/v1.ReplicaSetStatus`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReplicaSetStatus {
    pub observed_generation: u64,
    pub replicas: i32,
    pub ready_replicas: i32,
    pub available_replicas: i32,
}

/// `apps/v1.ReplicaSet`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReplicaSet {
    pub metadata: ObjectMeta,
    pub spec: ReplicaSetSpec,
    pub status: ReplicaSetStatus,
}

impl KubeResource for ReplicaSet {
    fn kind() -> &'static str {
        "ReplicaSet"
    }
    fn meta(&self) -> &ObjectMeta {
        &self.metadata
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.metadata
    }
}

/// `apps/v1.DaemonSetSpec`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DaemonSetSpec {
    pub selector: LabelSelector,
    pub template: PodTemplateSpec,
}

/// `apps/v1.DaemonSetStatus`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DaemonSetStatus {
    pub observed_generation: u64,
    pub current_number_scheduled: i32,
    pub desired_number_scheduled: i32,
    pub number_ready: i32,
}

/// `apps/v1.DaemonSet`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DaemonSet {
    pub metadata: ObjectMeta,
    pub spec: DaemonSetSpec,
    pub status: DaemonSetStatus,
}

impl KubeResource for DaemonSet {
    fn kind() -> &'static str {
        "DaemonSet"
    }
    fn meta(&self) -> &ObjectMeta {
        &self.metadata
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.metadata
    }
}

/// `apps/v1.StatefulSetSpec`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StatefulSetSpec {
    pub replicas: i32,
    pub selector: LabelSelector,
    pub template: PodTemplateSpec,
    pub service_name: String,
    /// `volumeClaimTemplates` — each PVC name is templated as
    /// `<vct>-<sts>-<ordinal>` per upstream `getPersistentVolumeClaimName`.
    pub volume_claim_templates: Vec<String>,
}

/// `apps/v1.StatefulSetStatus`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StatefulSetStatus {
    pub observed_generation: u64,
    pub replicas: i32,
    pub ready_replicas: i32,
    pub current_replicas: i32,
}

/// `apps/v1.StatefulSet`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StatefulSet {
    pub metadata: ObjectMeta,
    pub spec: StatefulSetSpec,
    pub status: StatefulSetStatus,
}

impl KubeResource for StatefulSet {
    fn kind() -> &'static str {
        "StatefulSet"
    }
    fn meta(&self) -> &ObjectMeta {
        &self.metadata
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.metadata
    }
}

// ---------------------------------------------------------------------------
// batch/v1 — Job, CronJob
// ---------------------------------------------------------------------------

/// `batch/v1.JobSpec`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JobSpec {
    pub parallelism: i32,
    pub completions: i32,
    pub backoff_limit: i32,
    pub selector: LabelSelector,
    pub template: PodTemplateSpec,
}

/// `batch/v1.JobStatus`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JobStatus {
    pub active: i32,
    pub succeeded: i32,
    pub failed: i32,
    pub completed: bool,
}

/// `batch/v1.Job`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Job {
    pub metadata: ObjectMeta,
    pub spec: JobSpec,
    pub status: JobStatus,
}

impl KubeResource for Job {
    fn kind() -> &'static str {
        "Job"
    }
    fn meta(&self) -> &ObjectMeta {
        &self.metadata
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.metadata
    }
}

/// `batch/v1.ConcurrencyPolicy`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ConcurrencyPolicy {
    #[default]
    Allow,
    Forbid,
    Replace,
}

/// `batch/v1.CronJobSpec`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CronJobSpec {
    /// Standard 5-field cron expression: `minute hour day-of-month month day-of-week`.
    pub schedule: String,
    pub concurrency_policy: ConcurrencyPolicy,
    pub job_template: JobSpec,
    pub successful_jobs_history_limit: i32,
    pub failed_jobs_history_limit: i32,
}

/// `batch/v1.CronJobStatus`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CronJobStatus {
    pub last_schedule_time_ms: Option<u64>,
    pub active_jobs: Vec<String>,
}

/// `batch/v1.CronJob`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CronJob {
    pub metadata: ObjectMeta,
    pub spec: CronJobSpec,
    pub status: CronJobStatus,
}

impl KubeResource for CronJob {
    fn kind() -> &'static str {
        "CronJob"
    }
    fn meta(&self) -> &ObjectMeta {
        &self.metadata
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.metadata
    }
}

// ---------------------------------------------------------------------------
// core/v1 — Namespace, Node, ServiceAccount, Secret (token), PVC
// ---------------------------------------------------------------------------

/// `core/v1.NamespacePhase`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NamespacePhase {
    #[default]
    Active,
    Terminating,
}

/// `core/v1.NamespaceStatus`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NamespaceStatus {
    pub phase: NamespacePhase,
}

/// `core/v1.Namespace`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Namespace {
    pub metadata: ObjectMeta,
    pub status: NamespaceStatus,
}

impl KubeResource for Namespace {
    fn kind() -> &'static str {
        "Namespace"
    }
    fn meta(&self) -> &ObjectMeta {
        &self.metadata
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.metadata
    }
}

/// `core/v1.NodeConditionType`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum NodeConditionType {
    Ready,
    MemoryPressure,
    DiskPressure,
    PIDPressure,
    NetworkUnavailable,
}

/// `core/v1.ConditionStatus`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ConditionStatus {
    True,
    False,
    #[default]
    Unknown,
}

/// `core/v1.NodeCondition`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeCondition {
    pub kind: NodeConditionType,
    pub status: ConditionStatus,
    pub reason: String,
    pub message: String,
    /// Unix-millis when the condition was last transitioned.
    pub last_transition_ms: u64,
    /// Unix-millis when the kubelet last heart-beated this condition.
    pub last_heartbeat_ms: u64,
}

/// `core/v1.Taint.Effect`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum TaintEffect {
    #[default]
    NoSchedule,
    PreferNoSchedule,
    NoExecute,
}

/// `core/v1.Taint`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Taint {
    pub key: String,
    pub value: String,
    pub effect: TaintEffect,
}

/// `core/v1.NodeSpec`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NodeSpec {
    pub unschedulable: bool,
    pub taints: Vec<Taint>,
}

/// `core/v1.NodeStatus`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NodeStatus {
    pub conditions: Vec<NodeCondition>,
}

impl NodeStatus {
    /// Find a condition by type.
    #[must_use]
    pub fn condition(&self, kind: NodeConditionType) -> Option<&NodeCondition> {
        self.conditions.iter().find(|c| c.kind == kind)
    }
}

/// `core/v1.Node`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Node {
    pub metadata: ObjectMeta,
    pub spec: NodeSpec,
    pub status: NodeStatus,
}

impl KubeResource for Node {
    fn kind() -> &'static str {
        "Node"
    }
    fn meta(&self) -> &ObjectMeta {
        &self.metadata
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.metadata
    }
}

/// `core/v1.ObjectReference` (truncated — the controllers consult
/// `name + namespace + uid`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ObjectReference {
    pub kind: String,
    pub namespace: String,
    pub name: String,
    pub uid: Uid,
}

/// `core/v1.ServiceAccount`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ServiceAccount {
    pub metadata: ObjectMeta,
    pub secrets: Vec<ObjectReference>,
}

impl KubeResource for ServiceAccount {
    fn kind() -> &'static str {
        "ServiceAccount"
    }
    fn meta(&self) -> &ObjectMeta {
        &self.metadata
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.metadata
    }
}

/// `core/v1.Secret` subset (the TokenController only handles
/// `kubernetes.io/service-account-token`-typed secrets).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Secret {
    pub metadata: ObjectMeta,
    /// Phase 2 stores the well-known token payload as opaque bytes.
    pub data: BTreeMap<String, Vec<u8>>,
    pub secret_type: String,
}

impl KubeResource for Secret {
    fn kind() -> &'static str {
        "Secret"
    }
    fn meta(&self) -> &ObjectMeta {
        &self.metadata
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.metadata
    }
}

/// `core/v1.PersistentVolumeClaim` — Phase 2 only models the StatefulSet-owned
/// flavour, so just metadata and the requested storage class is recorded.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PersistentVolumeClaim {
    pub metadata: ObjectMeta,
    pub storage_class: String,
}

impl KubeResource for PersistentVolumeClaim {
    fn kind() -> &'static str {
        "PersistentVolumeClaim"
    }
    fn meta(&self) -> &ObjectMeta {
        &self.metadata
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.metadata
    }
}

// ---------------------------------------------------------------------------
// Helpers shared by controllers
// ---------------------------------------------------------------------------

/// Make an [`OwnerReference`] pointing at `owner`. Sets `controller=true`.
///
/// Mirrors `metav1.NewControllerRef`.
#[must_use]
pub fn new_controller_ref<R: KubeResource>(owner: &R, api_version: &str) -> OwnerReference {
    OwnerReference {
        api_version: api_version.to_string(),
        kind: R::kind().to_string(),
        name: owner.name().to_string(),
        uid: owner.uid().clone(),
        controller: true,
        block_owner_deletion: true,
    }
}

/// Is `dependent` controlled by `owner`? Mirrors `metav1.IsControlledBy`.
#[must_use]
pub fn is_controlled_by(dependent: &ObjectMeta, owner: &Uid) -> bool {
    dependent
        .owner_references
        .iter()
        .any(|r| r.controller && &r.uid == owner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_selector_matches_when_subset() {
        let mut sel = LabelSelector::default();
        sel.match_labels.insert("app".into(), "nginx".into());
        let mut labels = BTreeMap::new();
        labels.insert("app".into(), "nginx".into());
        labels.insert("tier".into(), "frontend".into());
        assert!(sel.matches(&labels));
    }

    #[test]
    fn label_selector_does_not_match_when_value_differs() {
        let mut sel = LabelSelector::default();
        sel.match_labels.insert("app".into(), "nginx".into());
        let mut labels = BTreeMap::new();
        labels.insert("app".into(), "redis".into());
        assert!(!sel.matches(&labels));
    }

    #[test]
    fn empty_selector_is_empty() {
        let sel = LabelSelector::default();
        assert!(sel.is_empty());
    }

    #[test]
    fn is_controlled_by_finds_controller_ref() {
        let owner = Uid::new("u1");
        let mut meta = ObjectMeta::default();
        meta.owner_references.push(OwnerReference {
            uid: owner.clone(),
            controller: true,
            ..Default::default()
        });
        assert!(is_controlled_by(&meta, &owner));
    }

    #[test]
    fn is_controlled_by_ignores_non_controller_refs() {
        let owner = Uid::new("u1");
        let mut meta = ObjectMeta::default();
        meta.owner_references.push(OwnerReference {
            uid: owner.clone(),
            controller: false,
            ..Default::default()
        });
        assert!(!is_controlled_by(&meta, &owner));
    }
}
