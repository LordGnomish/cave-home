# Coverage matrix — cave-home-scheduler-rs

**Declared:** fill=0.095 · adr_justified=N/A · honest=N/A · port method per manifest.
**Verified:** 37/37 mapped symbols found in source · 83 test fns · drift: no.

## MAPPED (implemented + claimed)

| Spec capability | Source symbol | Verified |
|---|---|---|
| Scheduler struct (core controller) | src/scheduler.rs::Scheduler | yes |
| Single-pod scheduling driver | src/schedule_one.rs::schedule_one | yes |
| Scheduling result type | src/schedule_one.rs::ScheduleResult | yes |
| Filter plugin trait | src/framework/mod.rs::FilterPlugin | yes |
| Score plugin trait | src/framework/mod.rs::ScorePlugin | yes |
| PostFilter plugin trait | src/framework/mod.rs::PostFilterPlugin | yes |
| Status result type | src/framework/mod.rs::Status | yes |
| Status code enum | src/framework/mod.rs::Code | yes |
| Plugin cycle state | src/framework/cycle_state.rs::CycleState | yes |
| Plugin registry | src/framework/registry.rs::PluginRegistry | yes |
| Default plugin registry builder | src/plugins/mod.rs::default_registry | yes |
| NodeResourcesFit filter | src/plugins/node_resources_fit.rs::NodeResourcesFit | yes |
| NodeName filter | src/plugins/node_name.rs::NodeName | yes |
| NodeUnschedulable filter | src/plugins/node_unschedulable.rs::NodeUnschedulable | yes |
| NodePorts filter | src/plugins/node_ports.rs::NodePorts | yes |
| VolumeRestrictions filter | src/plugins/volume_restrictions.rs::VolumeRestrictions | yes |
| TaintToleration filter | src/plugins/taint_toleration.rs::TaintToleration | yes |
| NodeAffinity required-match filter | src/plugins/mod.rs::NodeAffinityFilter | yes |
| NodeResourcesBalancedAllocation scorer | src/plugins/node_resources_balanced.rs::NodeResourcesBalancedAllocation | yes |
| LeastRequested scorer | src/plugins/least_requested.rs::LeastRequested | yes |
| ImageLocality scorer | src/plugins/image_locality.rs::ImageLocality | yes |
| DefaultPreemption postfilter | src/preemption.rs::DefaultPreemption | yes |
| Scheduling queue trait | src/queue/mod.rs::SchedulingQueue | yes |
| Priority queue implementation | src/queue/priority_queue.rs::PriorityQueue | yes |
| Queued pod info | src/queue/priority_queue.rs::QueuedPodInfo | yes |
| Scheduler cache | src/cache/mod.rs::SchedulerCache | yes |
| Node info snapshot | src/cache/node_info.rs::NodeInfo | yes |
| Host port tracking | src/cache/node_info.rs::HostPortUse | yes |
| Assumed pod tracker | src/cache/assumed_pods.rs::AssumedPodTracker | yes |
| Pod type | src/types.rs::Pod | yes |
| Node type | src/types.rs::Node | yes |
| Taint type | src/types.rs::Taint | yes |
| Toleration type | src/types.rs::Toleration | yes |
| Node selector type | src/types.rs::NodeSelector | yes |
| Node selector requirement type | src/types.rs::NodeSelectorRequirement | yes |
| Affinity type | src/types.rs::Affinity | yes |
| Container port type | src/types.rs::ContainerPort | yes |
| Volume type | src/types.rs::Volume | yes |
| PVC source type | src/types.rs::PvcSource | yes |
| HostPath source type | src/types.rs::HostPathSource | yes |
| Resource requirements type | src/types.rs::ResourceRequirements | yes |
| Quantity type | src/types.rs::Quantity | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)

| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| PodTopologySpreadConstraints | phase-2b | Requires Pod/Node label indexing across snapshot. |
| Inter-pod affinity (required + preferred) | phase-2b | Complex cross-pod constraint matching. |
| NodeAffinity preferred terms | phase-2b | Score contribution only (required-match implemented). |
| Volume binding (static + dynamic PV/PVC) | phase-2b | Bind cycle and BindCompleted event handling. |
| Zone-aware volume scheduling | phase-2b | Zone constraint enforcement. |
| Per-node CSI/EBS/GCE-PD attach limits | phase-2b | Volume limit checking. |
| Dynamic Resource Allocation (DRA) | phase-2b | DRA beta (v1.32+) not in Phase 2 scope. |
| Reserve/Permit/PreBind extension points | phase-2b | Phase 2 ships Filter/Score/PostFilter only. |
| Multi-profile + custom plugin registry | phase-2b | Hard-coded default-scheduler profile in Phase 2. |
| ImageLocality image-size weighting | phase-2b | Phase 2 counts images only; size/coverage deferred. |
| DefaultPreemption victim minimization | phase-2b | First-fit victim set (not disruption-cost minimized). |
| Out-of-tree HTTP extenders | phase-2b | Legacy extension mechanism; framework plugins used. |
| Informer wiring (eventhandlers.go) | phase-2b | Replaced by SchedulerSource trait + drivers. |
| Multi-profile registry | phase-2b | Single profile shipped in Phase 2. |
| Prometheus metrics | phase-2b | Metric registration deferred. |
| Hint-driven queue re-queueing | phase-2b | Plain backoff only (v1.31+ hints not used). |
| Windows-only code | permanent | Linux 7.1+ only per Charter §3. |
| Pre-Linux 5.0 kernel fallbacks | permanent | Charter §3 mandates Linux 7.1+ baseline. |

## Drift notes
None — every claimed symbol exists in source.
