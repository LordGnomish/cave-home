# cave-home-scheduler-rs — event-loop port report (2026-06-07)

Branch: `feature/scheduler-event-loop` (worktree `../cave-home-scheduler-loop`,
based on `bec7a9a`). No push. Strict TDD, behavioural reimplementation of
`k8s.io/kubernetes/pkg/scheduler` (v1.36.1 reference).

## What was the blocker

The crate had a real decision core (queue+cache+filter/score/postfilter cycle +
preemption) but **no event-driven loop**: scheduling was driven by a manual
`sync()`/`run_once()` poll. There was no informer/watch integration, no
`unschedulablePods` set, no event-driven re-queue, no blocking pop, and the
framework stopped at Filter/Score/PostFilter. A pod that failed for lack of
capacity could never be woken by a later node-add. That is the Crit blocker this
work closes.

## Delivered (7 TDD cycles, each test→FAIL→impl→PASS)

| # | Cycle | Upstream reference |
|---|-------|--------------------|
| 1 | `ClusterEvent`/`GVK`/`ActionType` matching vocabulary | `framework/types.go` |
| 2 | `unschedulablePods` set + `AddUnschedulableIfNotPresent` (cycle-race routing) + `MoveAllToActiveOrBackoffQueue` + leftover flush | `backend/queue/scheduling_queue.go` |
| 3 | Blocking `pop_wait` + `close` (tokio `Notify`) | `PriorityQueue.Pop`/`Close` |
| 4 | Event-driven `Scheduler::run` + pod/node informers + flush ticker; `watch_nodes` on `SchedulerSource`; mock-apiserver integration tests | `scheduler.go::Run` + `eventhandlers.go` |
| 5 | `PreFilter` (node-subset + short-circuit + precompute) + `PreScore`; `NodeResourcesFit.PreFilter` | `framework/interface.go`, `noderesources/fit.go` |
| 6 | Binding cycle: `Reserve`→`Permit`→`PreBind`→`Bind` with `Unreserve` rollback | `framework/interface.go` + `runtime/framework.go` |
| 7 | `SchedulerConfig` `percentageOfNodesToScore` + adaptive `numFeasibleNodesToFind` | `apis/config` + `schedule_one.go` |

The framework cycle is now the full upstream shape:
`PreFilter → Filter → PostFilter → PreScore → Score → Reserve → Permit → PreBind → Bind`.

## Acceptance criteria

- **cargo test PASS** — 125 tests pass (was 91): 115 lib unit + 8 prior
  integration + 2 new mock-apiserver event-loop integration. `cargo clippy
  --lib` exits clean (net-zero new warnings vs the crate's pre-existing
  pedantic/nursery baseline of 67).
- **Integration test with a mock apiserver** — `tests/event_loop.rs` drives the
  real `Scheduler::run` loop against `InMemorySource`/`InMemorySink`:
  - `event_loop_binds_pod_streamed_after_start` — a pod published as a watch
    event after the loop starts is scheduled and bound.
  - `event_loop_node_add_unblocks_pending_pod` — a pod with no available node
    goes unschedulable, then a streamed **node-add** event fires
    `MoveAllToActiveOrBackoffQueue` and the pod is woken and bound. This is the
    event-driven behaviour the old poll driver could not provide.
- **Actual Pod-to-Node assignment** — both integration tests assert the concrete
  bind, e.g. `("default/beta", "n2")`, via the sink.
- **TDD git log compliance** — 7 `test(scheduler): … failing …` commits each
  immediately followed by its `feat(scheduler): …` implementation commit, plus a
  final `docs` commit. Each test commit was verified RED before the paired feat
  commit was made GREEN.

## LOC ratio

Production code is a behavioural reimplementation (the observable contract), not
a Go transcription, so a raw LOC ratio understates capability — Go is more
verbose and this Rust slice trades the reflective `runtime.Registry`/factory
machinery for static dispatch.

Added in this branch (base `bec7a9a` → HEAD), `cave-home-scheduler-rs`:

| Area | Added lines (incl. inline tests) |
|------|----------------------------------|
| `queue/priority_queue.rs` | +362 |
| `scheduler.rs` (run loop, informers, binding cycle) | +359 |
| `schedule_one.rs` (PreFilter/PreScore wiring + limit) | +211 |
| `framework/events.rs` (new) | +172 |
| `config.rs` (new) | +133 |
| `framework/registry.rs` | +110 |
| `plugins/node_resources_fit.rs` | +76 |
| `framework/mod.rs` (6 extension-point traits) | +63 |
| `source_sink.rs` (node watch) | +45 |
| `tests/event_loop.rs` (new) | +124 |
| **Total** | **≈ 1 655** added (src ≈ 1 531 + integration tests 124) |

Estimated split: ≈ 950 production LOC, ≈ 705 test LOC (inline unit + integration).

Approximate upstream reference for the ported subsystems (estimate; upstream not
vendored in this tree):

| Upstream file (relevant portion) | ~Go LOC |
|----------------------------------|---------|
| `backend/queue/scheduling_queue.go` (queue + unsched + moves + backoff) | ~1 100 |
| `eventhandlers.go` (informer event handlers) | ~700 |
| `scheduler.go` `Run` + bind glue (relevant slice) | ~200 |
| `framework/runtime/framework.go` `RunPreFilter/PreScore/Reserve/Permit/PreBind` | ~400 |
| `framework/types.go` GVK/ActionType/ClusterEvent | ~150 |
| `apis/config` + `numFeasibleNodesToFind` | ~120 |
| **Reference total (subsystems ported here)** | **≈ 2 670 Go LOC** |

**LOC ratio ≈ 950 production Rust / ≈ 2 670 Go ≈ 0.36** for the event-loop /
framework-cycle / config subsystems specifically. The crate-level capability
`fill_ratio` was lifted **0.28 → 0.42** (`honest_ratio` stays 1.00); manifest
`[[mapped]]`/`[[unmapped]]` updated accordingly.

## Still deferred (documented in `parity.manifest.toml`, ADR-004 Phase 2)

- `PostBind` extension point (no default-profile plugin uses it).
- `QueueingHints` (hint-driven precise re-queue; Phase 2 wakes on any matching
  cluster event, the upstream pre-hints behaviour).
- Multiple named profiles / reflective plugin registry (single
  `default-scheduler` profile).
- The concrete client-go `SharedInformerFactory` binding to a live apiserver —
  provided by the apiserver crate's shim; the scheduler consumes the narrow
  `SchedulerSource` watch traits.
- Plugins already deferred pre-this-work: PodTopologySpread, InterPodAffinity,
  VolumeBinding/Zone/Limits, DRA, preferred NodeAffinity scoring.

## Integration note

Work is isolated on `feature/scheduler-event-loop` in a dedicated worktree to
avoid racing the live committer on the shared checkout. A local `--no-ff` merge
into an integration branch off the base SHA records the integration without
pushing and without touching the live branch.
