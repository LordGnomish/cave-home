// SPDX-License-Identifier: Apache-2.0
//! `cave-home-scheduler-rs` — line-by-line Phase 2 port of the
//! Kubernetes scheduler (`pkg/scheduler/`).
//!
//! Upstream: kubernetes/kubernetes @ v1.36.1
//! SHA: `756939600b9a7180fc2df6550a4585b638875e67`
//! Subpath: `pkg/scheduler`
//!
//! Phase 2 scope (default scheduler profile only):
//!
//! * Filter plugins — `NodeUnschedulable`, `NodeName`, `NodeResourcesFit`,
//!   `NodePorts`, `VolumeRestrictions`, `TaintToleration`, `NodeAffinity`.
//! * Score plugins  — `NodeResourcesBalancedAllocation`, `LeastRequested`,
//!   `ImageLocality`.
//! * `DefaultPreemption` PostFilter plugin (priority-based).
//! * Priority `SchedulingQueue` with active + backoff sub-queues.
//! * `SchedulerCache` + `NodeInfo` aggregation + assumed-pod tracking.
//! * `scheduleOne` cycle plus a top-level `Scheduler` struct that wires
//!   it to `SchedulerSource` / `SchedulerSink` traits.
//!
//! Phase 2b deferred (see `parity.manifest.toml`): `PodTopologySpread`,
//! inter-pod affinity, preferred node affinity, custom plugin registry,
//! multiple profiles, `Reserve`/`Permit`/`PreBind` extension points,
//! image-size weighted `ImageLocality`, lower-priority victim
//! minimisation in `DefaultPreemption`.

pub mod cache;
pub mod framework;
pub mod plugins;
pub mod preemption;
pub mod queue;
pub mod schedule_one;
pub mod scheduler;
pub mod source_sink;
pub mod types;

pub use cache::{NodeInfo, SchedulerCache};
pub use framework::{
    ActionType, ClusterEvent, CycleState, Gvk, PermitPlugin, PluginRegistry, PreBindPlugin,
    PreFilterPlugin, PreFilterResult, PreScorePlugin, RegistryBuilder, ReservePlugin, Status,
};
pub use plugins::default_registry;
pub use preemption::DefaultPreemption;
pub use queue::{PriorityQueue, QueuedPodInfo, SchedulingQueue};
pub use schedule_one::{schedule_one, ScheduleResult};
pub use scheduler::{CycleOutcome, Scheduler};
pub use source_sink::{
    EventStream, InMemorySink, InMemorySource, NodeEvent, NodeEventStream, PodEvent, SchedulerSink,
    SchedulerSource, SourceSinkError,
};
pub use types::{
    Affinity, Container, ContainerPort, HostPathSource, Node, NodeAffinity, NodeSelector,
    NodeSelectorOperator, NodeSelectorRequirement, NodeSelectorTerm, NodeSpec, NodeStatus,
    ObjectMeta, Pod, PodPhase, PodSpec, PodStatus, Protocol, PvcSource, Quantity, ResourceList,
    ResourceName, ResourceRequirements, Taint, TaintEffect, Toleration, TolerationOperator,
    Volume, VolumeSource,
};
