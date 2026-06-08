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
//! * Priority `SchedulingQueue` with active + backoff sub-queues, an
//!   `unschedulablePods` set, event-driven `MoveAllToActiveOrBackoffQueue`
//!   (driven by `ClusterEvent`s), leftover flush, and a blocking `pop_wait`.
//! * `SchedulerCache` + `NodeInfo` aggregation + assumed-pod tracking.
//! * The full framework extension chain (all 9 points) — `PreFilter → Filter →
//!   PostFilter → PreScore → Score(+`NormalizeScore`)` (scheduling) and
//!   `Reserve → Permit → PreBind → Bind → PostBind` with `Unreserve` rollback
//!   (binding). `Bind` is a real extension point: registered `BindPlugin`s run
//!   in order, the first non-`Skip` owns the bind, else the built-in
//!   `DefaultBinder` (the `SchedulerSink` Binding POST) handles it.
//! * Event-driven `Scheduler::run` loop with pod/node informers over the
//!   `SchedulerSource` watch streams and a periodic backoff/leftover flush,
//!   plus the legacy `sync`/`run_once` poll driver (which now also drives the
//!   full binding cycle).
//! * `SchedulerConfig` — `percentageOfNodesToScore` + adaptive
//!   `numFeasibleNodesToFind`.
//!
//! Phase 2b deferred (see `parity.manifest.toml`): `PodTopologySpread`,
//! inter-pod affinity, preferred node affinity, custom plugin registry,
//! multiple profiles, the timed `Permit` "wait" disposition, `QueueingHints`,
//! image-size weighted `ImageLocality`, lower-priority victim
//! minimisation in `DefaultPreemption`.

pub mod cache;
pub mod config;
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
    ActionType, BindPlugin, ClusterEvent, Code, CycleState, FilterPlugin, Gvk, NodeScore,
    PermitDecision, PermitPlugin, PluginRegistry, PostBindPlugin, PostFilterPlugin, PreBindPlugin,
    PreEnqueuePlugin, PreFilterExtensions, PreFilterPlugin, PreFilterResult, PreScorePlugin,
    QueueSortPlugin, RegistryBuilder, ReservePlugin, ScorePlugin, Status, WaitingPod,
};
pub use plugins::default_registry;
pub use preemption::DefaultPreemption;
pub use queue::{PriorityQueue, QueuedPodInfo, SchedulingQueue};
pub use config::SchedulerConfig;
pub use schedule_one::{schedule_one, schedule_one_limited, ScheduleResult};
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
