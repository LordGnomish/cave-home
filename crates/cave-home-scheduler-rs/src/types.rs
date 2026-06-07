// SPDX-License-Identifier: Apache-2.0
//! Minimal subset of `k8s.io/api/core/v1` needed by the scheduler.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//!         staging/src/k8s.io/api/core/v1/types.go
//!
//! Only the fields touched by the Phase 2 default plugin set are modelled.
//! Larger surface (TopologySpread keys, preferred affinity, claims, resize
//! status, …) is deliberately deferred — see `parity.manifest.toml`.

use std::collections::BTreeMap;

/// Upstream: `k8s.io/apimachinery/pkg/types.UID`.
pub type Uid = String;

/// Upstream: `k8s.io/apimachinery/pkg/types.NamespacedName.String()`.
#[must_use]
pub fn namespaced(namespace: &str, name: &str) -> String {
    format!("{namespace}/{name}")
}

// ---------- ResourceList -----------------------------------------------------

/// Upstream: `k8s.io/api/core/v1.ResourceName`. Phase 2 needs only CPU and
/// memory; ephemeral-storage / hugepages / GPU live in `[[unmapped]]`.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum ResourceName {
    Cpu,
    Memory,
}

/// Upstream: `k8s.io/apimachinery/pkg/api/resource.Quantity`.
/// Internally an integer (milli-CPU for CPU, bytes for memory).
#[derive(Debug, Clone, Copy, Eq, PartialEq, Default, Hash)]
pub struct Quantity(pub i64);

impl Quantity {
    #[must_use]
    pub const fn milli_cpu(milli: i64) -> Self {
        Self(milli)
    }

    /// Memory quantity in bytes.
    #[must_use]
    pub const fn bytes(b: i64) -> Self {
        Self(b)
    }

    #[must_use]
    pub const fn value(self) -> i64 {
        self.0
    }
}

/// Upstream: `k8s.io/api/core/v1.ResourceList`.
pub type ResourceList = BTreeMap<ResourceName, Quantity>;

/// Upstream: `k8s.io/api/core/v1.ResourceRequirements`.
#[derive(Debug, Clone, Default)]
pub struct ResourceRequirements {
    pub requests: ResourceList,
    pub limits: ResourceList,
}

// ---------- Container --------------------------------------------------------

/// Upstream: `k8s.io/api/core/v1.Protocol`.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum Protocol {
    Tcp,
    Udp,
    Sctp,
}

impl Default for Protocol {
    fn default() -> Self {
        Self::Tcp
    }
}

/// Upstream: `k8s.io/api/core/v1.ContainerPort`.
#[derive(Debug, Clone, Default)]
pub struct ContainerPort {
    pub host_port: i32,
    pub container_port: i32,
    pub protocol: Protocol,
    pub host_ip: String,
}

/// Upstream: `k8s.io/api/core/v1.Container`.
#[derive(Debug, Clone, Default)]
pub struct Container {
    pub name: String,
    pub image: String,
    pub resources: ResourceRequirements,
    pub ports: Vec<ContainerPort>,
}

// ---------- Volume -----------------------------------------------------------

/// Upstream: `k8s.io/api/core/v1.PersistentVolumeClaimVolumeSource`.
#[derive(Debug, Clone, Default)]
pub struct PvcSource {
    pub claim_name: String,
    pub read_only: bool,
}

/// Upstream: `k8s.io/api/core/v1.HostPathVolumeSource`.
#[derive(Debug, Clone, Default)]
pub struct HostPathSource {
    pub path: String,
}

/// Upstream: `k8s.io/api/core/v1.VolumeSource`. Phase 2 only models the
/// variants that participate in `VolumeRestrictions` filtering.
#[derive(Debug, Clone)]
pub enum VolumeSource {
    EmptyDir,
    HostPath(HostPathSource),
    PersistentVolumeClaim(PvcSource),
}

/// Upstream: `k8s.io/api/core/v1.Volume`.
#[derive(Debug, Clone)]
pub struct Volume {
    pub name: String,
    pub source: VolumeSource,
}

// ---------- Affinity / NodeSelector -----------------------------------------

/// Upstream: `k8s.io/api/core/v1.NodeSelectorOperator`.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum NodeSelectorOperator {
    In,
    NotIn,
    Exists,
    DoesNotExist,
}

/// Upstream: `k8s.io/api/core/v1.NodeSelectorRequirement`.
#[derive(Debug, Clone, Default)]
pub struct NodeSelectorRequirement {
    pub key: String,
    pub operator: Option<NodeSelectorOperator>,
    pub values: Vec<String>,
}

impl NodeSelectorRequirement {
    /// Upstream: `pkg/apis/core/v1/helper/helpers.go::NodeSelectorRequirementsAsSelector`
    /// (per-requirement match against a label map).
    #[must_use]
    pub fn matches(&self, labels: &BTreeMap<String, String>) -> bool {
        let op = self.operator.unwrap_or(NodeSelectorOperator::Exists);
        match op {
            NodeSelectorOperator::In => labels
                .get(&self.key)
                .is_some_and(|v| self.values.iter().any(|w| w == v)),
            NodeSelectorOperator::NotIn => labels
                .get(&self.key)
                .is_none_or(|v| !self.values.iter().any(|w| w == v)),
            NodeSelectorOperator::Exists => labels.contains_key(&self.key),
            NodeSelectorOperator::DoesNotExist => !labels.contains_key(&self.key),
        }
    }
}

/// Upstream: `k8s.io/api/core/v1.NodeSelectorTerm`.
#[derive(Debug, Clone, Default)]
pub struct NodeSelectorTerm {
    pub match_expressions: Vec<NodeSelectorRequirement>,
}

/// Upstream: `k8s.io/api/core/v1.NodeSelector`.
/// The terms are OR'd, and within a term, the expressions are AND'd.
#[derive(Debug, Clone, Default)]
pub struct NodeSelector {
    pub node_selector_terms: Vec<NodeSelectorTerm>,
}

impl NodeSelector {
    /// Upstream: `pkg/apis/core/v1/helper/helpers.go::MatchNodeSelectorTerms`.
    #[must_use]
    pub fn matches(&self, labels: &BTreeMap<String, String>) -> bool {
        if self.node_selector_terms.is_empty() {
            return false;
        }
        self.node_selector_terms.iter().any(|t| {
            t.match_expressions.iter().all(|r| r.matches(labels))
        })
    }
}

/// Upstream: `k8s.io/api/core/v1.PreferredSchedulingTerm`. A weighted soft
/// node-affinity preference: nodes matching `preference` gain `weight` score.
#[derive(Debug, Clone, Default)]
pub struct PreferredSchedulingTerm {
    /// Weight in `1..=100` (validation is the apiserver's job; the scheduler
    /// sums whatever it is given).
    pub weight: i32,
    /// The node selector term a node must match to earn `weight`.
    pub preference: NodeSelectorTerm,
}

/// Upstream: `k8s.io/api/core/v1.NodeAffinity`. Phase 2 honours the hard
/// `required_during_scheduling_ignored_during_execution` (Filter) and the soft
/// `preferred_during_scheduling_ignored_during_execution` (Score).
#[derive(Debug, Clone, Default)]
pub struct NodeAffinity {
    pub required_during_scheduling: Option<NodeSelector>,
    pub preferred_during_scheduling: Vec<PreferredSchedulingTerm>,
}

/// Upstream: `k8s.io/api/core/v1.Affinity`.
#[derive(Debug, Clone, Default)]
pub struct Affinity {
    pub node_affinity: Option<NodeAffinity>,
}

// ---------- Taints / Tolerations --------------------------------------------

/// Upstream: `k8s.io/api/core/v1.TaintEffect`.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum TaintEffect {
    NoSchedule,
    PreferNoSchedule,
    NoExecute,
}

/// Upstream: `k8s.io/api/core/v1.Taint`.
#[derive(Debug, Clone)]
pub struct Taint {
    pub key: String,
    pub value: String,
    pub effect: TaintEffect,
}

/// Upstream: `k8s.io/api/core/v1.TolerationOperator`.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum TolerationOperator {
    Equal,
    Exists,
}

impl Default for TolerationOperator {
    fn default() -> Self {
        Self::Equal
    }
}

/// Upstream: `k8s.io/api/core/v1.Toleration`.
#[derive(Debug, Clone, Default)]
pub struct Toleration {
    pub key: String,
    pub operator: TolerationOperator,
    pub value: String,
    pub effect: Option<TaintEffect>,
}

impl Toleration {
    /// Upstream: `pkg/apis/core/v1/helper/helpers.go::Toleration.ToleratesTaint`.
    #[must_use]
    pub fn tolerates(&self, taint: &Taint) -> bool {
        if let Some(e) = self.effect {
            if e != taint.effect {
                return false;
            }
        }
        match self.operator {
            TolerationOperator::Exists => self.key.is_empty() || self.key == taint.key,
            TolerationOperator::Equal => self.key == taint.key && self.value == taint.value,
        }
    }
}

// ---------- ObjectMeta / Pod / Node -----------------------------------------

/// Upstream: `k8s.io/apimachinery/pkg/apis/meta/v1.ObjectMeta`.
/// Only the fields the scheduler reads.
#[derive(Debug, Clone, Default)]
pub struct ObjectMeta {
    pub name: String,
    pub namespace: String,
    pub uid: Uid,
    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,
}

/// Upstream: `k8s.io/api/core/v1.PodSpec` (scheduler-relevant subset).
#[derive(Debug, Clone, Default)]
pub struct PodSpec {
    pub containers: Vec<Container>,
    pub node_name: String,
    pub node_selector: BTreeMap<String, String>,
    pub affinity: Option<Affinity>,
    pub tolerations: Vec<Toleration>,
    pub volumes: Vec<Volume>,
    pub priority: i32,
    pub scheduler_name: String,
}

/// Upstream: `k8s.io/api/core/v1.PodPhase`.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum PodPhase {
    Pending,
    Running,
    Succeeded,
    Failed,
    Unknown,
}

impl Default for PodPhase {
    fn default() -> Self {
        Self::Pending
    }
}

/// Upstream: `k8s.io/api/core/v1.PodStatus` (subset).
#[derive(Debug, Clone, Default)]
pub struct PodStatus {
    pub phase: PodPhase,
    pub message: String,
    pub host_ip: String,
}

/// Upstream: `k8s.io/api/core/v1.Pod`.
#[derive(Debug, Clone, Default)]
pub struct Pod {
    pub metadata: ObjectMeta,
    pub spec: PodSpec,
    pub status: PodStatus,
}

impl Pod {
    /// Upstream `pkg/scheduler/util/utils.go::GetPodFullName`.
    #[must_use]
    pub fn full_name(&self) -> String {
        namespaced(&self.metadata.namespace, &self.metadata.name)
    }

    /// Sum of `requests` across all containers for a single resource.
    ///
    /// Upstream: `pkg/scheduler/framework/types.go::calculateResource`
    /// (single-resource fast path). Init-container max() is deferred.
    #[must_use]
    pub fn sum_requests(&self, name: ResourceName) -> Quantity {
        let mut total = 0i64;
        for c in &self.spec.containers {
            if let Some(q) = c.resources.requests.get(&name) {
                total = total.saturating_add(q.0);
            }
        }
        Quantity(total)
    }
}

/// Upstream: `k8s.io/api/core/v1.NodeSpec` (subset).
#[derive(Debug, Clone, Default)]
pub struct NodeSpec {
    pub unschedulable: bool,
    pub taints: Vec<Taint>,
}

/// Upstream: `k8s.io/api/core/v1.NodeStatus` (subset).
#[derive(Debug, Clone, Default)]
pub struct NodeStatus {
    pub capacity: ResourceList,
    pub allocatable: ResourceList,
    pub images: Vec<String>,
}

/// Upstream: `k8s.io/api/core/v1.Node`.
#[derive(Debug, Clone, Default)]
pub struct Node {
    pub metadata: ObjectMeta,
    pub spec: NodeSpec,
    pub status: NodeStatus,
}

impl Node {
    #[must_use]
    pub fn allocatable(&self, name: ResourceName) -> Quantity {
        self.status.allocatable.get(&name).copied().unwrap_or_default()
    }

    #[must_use]
    pub fn has_image(&self, image: &str) -> bool {
        self.status.images.iter().any(|i| i == image)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rl(cpu_m: i64, mem_b: i64) -> ResourceList {
        let mut r = ResourceList::new();
        r.insert(ResourceName::Cpu, Quantity::milli_cpu(cpu_m));
        r.insert(ResourceName::Memory, Quantity::bytes(mem_b));
        r
    }

    #[test]
    fn pod_full_name_is_namespace_slash_name() {
        let mut p = Pod::default();
        p.metadata.namespace = "kube-system".into();
        p.metadata.name = "kube-dns".into();
        assert_eq!(p.full_name(), "kube-system/kube-dns");
    }

    #[test]
    fn pod_sum_requests_aggregates_containers() {
        let mut p = Pod::default();
        let mut c1 = Container::default();
        c1.resources.requests = rl(100, 200);
        let mut c2 = Container::default();
        c2.resources.requests = rl(250, 0);
        p.spec.containers.push(c1);
        p.spec.containers.push(c2);
        assert_eq!(p.sum_requests(ResourceName::Cpu), Quantity(350));
        assert_eq!(p.sum_requests(ResourceName::Memory), Quantity(200));
    }

    #[test]
    fn node_selector_term_or_match_expression_and() {
        let mut labels = BTreeMap::new();
        labels.insert("zone".into(), "us-east-1a".into());
        labels.insert("role".into(), "worker".into());

        let req_zone = NodeSelectorRequirement {
            key: "zone".into(),
            operator: Some(NodeSelectorOperator::In),
            values: vec!["us-east-1a".into(), "us-east-1b".into()],
        };
        let req_role = NodeSelectorRequirement {
            key: "role".into(),
            operator: Some(NodeSelectorOperator::In),
            values: vec!["worker".into()],
        };
        let ns = NodeSelector {
            node_selector_terms: vec![NodeSelectorTerm {
                match_expressions: vec![req_zone, req_role],
            }],
        };
        assert!(ns.matches(&labels));

        let mut other = BTreeMap::new();
        other.insert("zone".into(), "us-west-2".into());
        assert!(!ns.matches(&other));
    }

    #[test]
    fn empty_node_selector_does_not_match() {
        let ns = NodeSelector::default();
        let labels = BTreeMap::new();
        assert!(!ns.matches(&labels));
    }

    #[test]
    fn node_selector_exists_and_does_not_exist() {
        let mut labels = BTreeMap::new();
        labels.insert("k".into(), "v".into());

        let exists = NodeSelectorRequirement {
            key: "k".into(),
            operator: Some(NodeSelectorOperator::Exists),
            values: vec![],
        };
        let dne = NodeSelectorRequirement {
            key: "m".into(),
            operator: Some(NodeSelectorOperator::DoesNotExist),
            values: vec![],
        };
        assert!(exists.matches(&labels));
        assert!(dne.matches(&labels));
    }

    #[test]
    fn toleration_equal_tolerates_matching_taint() {
        let taint = Taint {
            key: "key".into(),
            value: "v".into(),
            effect: TaintEffect::NoSchedule,
        };
        let tol = Toleration {
            key: "key".into(),
            operator: TolerationOperator::Equal,
            value: "v".into(),
            effect: Some(TaintEffect::NoSchedule),
        };
        assert!(tol.tolerates(&taint));

        let wrong_val = Toleration {
            value: "other".into(),
            ..tol.clone()
        };
        assert!(!wrong_val.tolerates(&taint));
    }

    #[test]
    fn toleration_exists_tolerates_any_value_matching_key() {
        let taint = Taint {
            key: "key".into(),
            value: "anything".into(),
            effect: TaintEffect::NoSchedule,
        };
        let tol = Toleration {
            key: "key".into(),
            operator: TolerationOperator::Exists,
            value: String::new(),
            effect: Some(TaintEffect::NoSchedule),
        };
        assert!(tol.tolerates(&taint));
    }

    #[test]
    fn toleration_exists_with_empty_key_matches_anything() {
        let taint = Taint {
            key: "anything".into(),
            value: "v".into(),
            effect: TaintEffect::NoSchedule,
        };
        let tol = Toleration {
            key: String::new(),
            operator: TolerationOperator::Exists,
            value: String::new(),
            effect: Some(TaintEffect::NoSchedule),
        };
        assert!(tol.tolerates(&taint));
    }

    #[test]
    fn toleration_effect_mismatch_does_not_tolerate() {
        let taint = Taint {
            key: "k".into(),
            value: "v".into(),
            effect: TaintEffect::NoSchedule,
        };
        let tol = Toleration {
            key: "k".into(),
            operator: TolerationOperator::Equal,
            value: "v".into(),
            effect: Some(TaintEffect::NoExecute),
        };
        assert!(!tol.tolerates(&taint));
    }

    #[test]
    fn node_has_image_returns_true_for_present_image() {
        let mut n = Node::default();
        n.status.images = vec!["nginx:1.27".into(), "redis:7".into()];
        assert!(n.has_image("redis:7"));
        assert!(!n.has_image("postgres:16"));
    }
}
