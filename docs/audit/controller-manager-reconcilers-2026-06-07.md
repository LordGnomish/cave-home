# controller-manager reconciler uplift — 2026-06-07

Branch: `feature/controller-mgr-reconcilers` (worktree `../cave-home-controller-mgr`,
based on `bec7a9a`). Strict TDD, no push.

## What this added

The crate `cave-home-controller-manager-rs` was a `std`-only **decision core**
(workqueue + reconcile policy + informer Store/DeltaFifo + 3 pure controllers:
GC, node-lifecycle, cleanup). This uplift adds the **real reconciler controllers
+ in-memory apiserver + manager run loop + leader election**.

### New modules (`src/`)

- `apis/` — typed workload object model (`core.rs`: Pod, Service, Namespace,
  Node, Endpoints, ServiceAccount; `apps.rs`: ReplicaSet, Deployment,
  StatefulSet, DaemonSet; `batch.rs`: Job, CronJob) **+ the in-memory apiserver**
  `client.rs` (`Api<T>` + `Cluster`). `Api<T>` is the client-go
  `fake.Clientset`/`ObjectTracker` analogue — a *real* in-memory implementation
  of create (UID assignment) / get / update / delete / list / list_matching /
  list_owned_by, **not a stub**. Also `template_hash` (FNV-1a pod-template-hash)
  and `selector_matches`.
- `manager.rs` — `Manager`: a `WorkQueue` per controller + a resync-driven step
  loop (reconcile Deployments → reconcile ReplicaSets → admit pods → reconcile
  back up), driving controllers via `apply_outcome`. `admit_pods` simulates
  kubelet so availability propagates and rollouts converge.
- `leaderelection.rs` — `coordination.k8s.io/v1` Lease + `try_acquire_or_renew`
  (acquire empty / renew own / take over expired / lose to valid holder, bumping
  `leaseTransitions`) + a stateful `LeaderElector`.

### New controllers (`src/controllers/`, real reconcilers over `Cluster`)

`replicaset`, `deployment`, `statefulset`, `daemonset`, `job`, `cronjob`,
`namespace`, `serviceaccount`, `endpoints`. (Plus the original `gc`,
`node_lifecycle`, `cleanup` = **12 controllers**.) The `Object` trait gained
`meta_mut` so the apiserver can stamp UIDs and controllers can set owner-refs.

### Tests (`tests/`, 1 file per controller, RED→GREEN)

`apiserver`, `replicaset_controller`, `deployment_controller`, `e2e_deployment`,
`statefulset_controller`, `daemonset_controller`, `job_controller`,
`cronjob_controller`, `namespace_controller`, `serviceaccount_controller`,
`endpoints_controller`, `leader_election`.

**Acceptance met:** `cargo test` PASS (140 total: 72 lib + 67 integration + 1
doctest); integration tests run against the in-memory (mock) apiserver; the
`e2e_deployment` test proves the **Deployment → ReplicaSet → Pod** flow (create,
scale up/down, full rolling update v1→v2) converges through the work-queue loop;
`leader_election` proves multi-instance at-most-one-leader + expiry takeover.
Lib is `cargo clippy --lib` warning-clean (pedantic+nursery). LOC: lib
2220 → 4461 (+~2240), 1287 LOC of integration tests.

## Design note — why this is honest, not a phantom backend

The manifest had deferred the workload controllers precisely because a *prior*
revision stubbed them "against an unreal async client." This uplift avoids that
trap by building a genuinely-functional in-memory apiserver (the fake-clientset
pattern all upstream controller tests use) and reconciling against it. Only the
**networked transport** (REST clientset + watch reflector) remains deferred —
the controllers are transport-agnostic and would bind to the real client
unchanged.

## What remains deferred (see `parity.manifest.toml` `[[unmapped]]`)

- Networked REST clientset + watch reflector (resourceVersion optimistic
  concurrency) — lands with the apiserver crate.
- `EndpointSlice` sharding + per-port subsets (core Endpoints IS done).
- HPA, ResourceQuota, PodDisruptionBudget, volume (PV/PVC), certificates,
  bootstrap, cloud/node-IPAM controllers.
- CronJob cron-*expression* parsing (period is consumed pre-resolved).
- controller-manager Prometheus metrics.

`fill_ratio` 0.18 → 0.45, `honest_ratio` 1.00.
