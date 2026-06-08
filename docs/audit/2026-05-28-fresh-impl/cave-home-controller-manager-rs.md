# Coverage matrix — cave-home-controller-manager-rs

**Declared:** fill=0.054 · adr_justified=0.054 · honest=0.054 (Phase 2 MVP core controllers only).
**Verified:** 38/46 mapped symbols found in source · 79 test fns · drift: **YES** — 7 claimed method symbols absent.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| ControllerManager bootstrap | src/manager.rs::ControllerManager | yes |
| RateLimitingQueue workqueue | src/workqueue.rs::RateLimitingQueue | yes |
| WorkQueue (base queue) | src/workqueue.rs::WorkQueue | yes |
| ExponentialBackoffRateLimiter | src/workqueue.rs::ExponentialBackoffRateLimiter | yes |
| SharedInformer cache | src/informer.rs::SharedInformer | yes |
| Reflector store sync | src/informer.rs::Reflector | yes |
| Local object store | src/informer.rs::Store | yes |
| API client interface | src/api_client.rs::ControllerApiClient | yes |
| DeploymentController | src/controllers/deployment.rs::DeploymentController | yes |
| Deployment reconcile | src/controllers/deployment.rs::DeploymentController::reconcile | yes |
| Deployment rolling logic | src/controllers/deployment.rs::DeploymentController::rollout_rolling | **NO** |
| ReplicaSetController | src/controllers/replicaset.rs::ReplicaSetController | yes |
| ReplicaSet reconcile | src/controllers/replicaset.rs::ReplicaSetController::reconcile | yes |
| ReplicaSet replica management | src/controllers/replicaset.rs::ReplicaSetController::manage_replicas | **NO** |
| DaemonSetController | src/controllers/daemonset.rs::DaemonSetController | yes |
| DaemonSet reconcile | src/controllers/daemonset.rs::DaemonSetController::reconcile | yes |
| DaemonSet node scheduling | src/controllers/daemonset.rs::node_should_run | **NO** |
| StatefulSetController | src/controllers/statefulset.rs::StatefulSetController | yes |
| StatefulSet reconcile | src/controllers/statefulset.rs::StatefulSetController::reconcile | yes |
| StatefulSet ordered pod creation | src/controllers/statefulset.rs::StatefulSetController::ensure_ordered_pods | **NO** |
| JobController | src/controllers/job.rs::JobController | yes |
| Job reconcile | src/controllers/job.rs::JobController::reconcile | yes |
| Job pod management | src/controllers/job.rs::JobController::manage_pods | **NO** |
| CronJobController | src/controllers/cronjob.rs::CronJobController | yes |
| CronJob reconcile | src/controllers/cronjob.rs::CronJobController::reconcile | yes |
| Cron schedule parsing | src/controllers/cronjob.rs::next_schedule_time | **NO** |
| ServiceAccountController | src/controllers/serviceaccount.rs::ServiceAccountController | yes |
| TokenController | src/controllers/serviceaccount.rs::TokenController | yes |
| NamespaceController | src/controllers/namespace.rs::NamespaceController | yes |
| Namespace cascade delete | src/controllers/namespace.rs::NamespaceController::cascade_delete | **NO** |
| NodeController | src/controllers/node.rs::NodeController | yes |
| Node health monitoring | src/controllers/node.rs::NodeController::reconcile | yes |
| GarbageCollector | src/controllers/garbage_collector.rs::GarbageCollector | yes |
| Ownership graph | src/controllers/garbage_collector.rs::OwnerGraph | yes |
| Graph change processing | src/controllers/garbage_collector.rs::GarbageCollector::process_graph_changes | **NO** |
| ObjectMeta type | src/types.rs::ObjectMeta | yes |
| OwnerReference type | src/types.rs::OwnerReference | yes |
| LabelSelector type | src/types.rs::LabelSelector | yes |
| Deployment type | src/types.rs::Deployment | yes |
| ReplicaSet type | src/types.rs::ReplicaSet | yes |
| DaemonSet type | src/types.rs::DaemonSet | yes |
| StatefulSet type | src/types.rs::StatefulSet | yes |
| Job type | src/types.rs::Job | yes |
| CronJob type | src/types.rs::CronJob | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| HorizontalPodAutoscaler | phase-2b | Deferred to Phase 2b; depends on metrics-server integration |
| EndpointController | phase-2b | Deferred to Phase 2b; will integrate with cave-home-kube-proxy-rs |
| EndpointSliceController | phase-2b | Deferred to Phase 2b |
| ResourceQuotaController | phase-2b | Deferred to Phase 2b |
| LimitRangeController | phase-2b | Deferred to Phase 2b |
| ServiceController (cloud LB) | phase-2b | Deferred to Phase 2b; home cluster has no cloud load-balancer |
| PersistentVolumeController | phase-2b | Deferred to Phase 2b alongside VolumeBinding scheduler plugin |
| PVCProtectionController | phase-2b | Deferred to Phase 2b |
| TTLController | phase-2b | Deferred to Phase 2b |
| BootstrapSignerController | phase-2b | Deferred to Phase 2b |
| CertificateSigningRequestController | phase-2b | Deferred to Phase 2b |
| DisruptionController (PDB) | phase-2b | Deferred to Phase 2b |
| ReplicationController (legacy RC) | permanent | Deferred indefinitely; superseded by ReplicaSet |

## Drift notes

**Critical:** 7 mapped method symbols are declared but not found in source. This indicates either:

1. **Incomplete implementation** — The methods are claimed in the manifest but the actual impl blocks only have `reconcile` (+ constructors). The logic exists as standalone functions (e.g., `sync_deployment`, `sync_replica_set`, `sync_daemon_set`) instead of methods on the controller structs.

2. **Manifest lag** — The manifest was written to describe a planned structure where every significant sync operation would be a method, but the implementation uses standalone async functions and delegates from `reconcile` to them.

**Specific missing methods:**
- `src/controllers/deployment.rs::DeploymentController::rollout_rolling` — rolling logic is in standalone `sync_deployment` function
- `src/controllers/replicaset.rs::ReplicaSetController::manage_replicas` — replica reconciliation is in standalone `sync_replica_set` function
- `src/controllers/daemonset.rs::node_should_run` — node scheduling is in standalone `sync_daemon_set` function
- `src/controllers/statefulset.rs::StatefulSetController::ensure_ordered_pods` — ordered reconciliation is in standalone `sync_stateful_set` function
- `src/controllers/job.rs::JobController::manage_pods` — pod management is in standalone `sync_job` function
- `src/controllers/cronjob.rs::next_schedule_time` — cron parsing is in the `CronSchedule::next_after` method, not a top-level function
- `src/controllers/garbage_collector.rs::GarbageCollector::process_graph_changes` — graph processing is in `build_graph` standalone function; no `process_graph_changes` method exists anywhere

**Other drift:** `src/controllers/namespace.rs::NamespaceController::cascade_delete` is claimed to be on NamespaceController but the implementation delegates to `GarbageCollector::cascade_delete` method which does exist.

All type definitions verified as present and correct. All controller structs verified. All 79 tests (#[test] + #[tokio::test]) accounted for.
