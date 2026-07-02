# cave-home sweep report 2026-06-16

48-hour autopilot sweep: open the 2026-06-09 merge stall, land every finished
branch on `main` test-gated, fix the two charter violations, refresh honest
measurement, prune the branch/worktree debris.

## Headline

| Metric | Before | After |
|---|---|---|
| Workspace tests | 3,884 pass / 0 fail | **5,795 pass / 0 fail** |
| `main` | `f1a231a` | `d81f4fd` (pushed) |
| Merges landed | — | 26 (every one test-gated, main never red at a push point) |
| Parity index | 62 crates, 14 null honest_ratio, mean fill 0.32 | **69 crates, 0 null, mean fill 0.373** |
| Local branches | 84 (+ 36 worktrees) | 2 (main + 1 follow-up), 1 worktree |
| Remote branches | 68 | 2 (main + 1 follow-up) |

## Day 1 — merge stall opened

Root cause confirmed: the cont3 wave failed on 2026-06-09 because its
integration tests depend on `orchestration::metrics_server` +
`local_path_provisioner`, which were never merged. The untracked
`src/metrics_server/` on main was a **partial copy** (1 of 6 documented
modules) — deleted; the full module merged from its branch.

1. `25b8f1d` metrics_server module + cavehomectl top + portal Metrics (+76)
2. `884c26a` lpp-port: local_path_provisioner + ServiceLB reconcile; red-left
   `ServiceLbMetrics` TDD test fixed by cherry-picking its green impl
   (`19347fd`, `0eeb160`) (+74)
3. `f022abe` kine-cont3: real backend (rusqlite/tonic/pg/mysql), Lease gRPC,
   compaction, cancellable watch (+249)
4. `f695db4` scheduler-cont3: event-driven run loop + informers + 9/9
   framework chain (+55)
5. `6c739a7` controller-mgr-cont3: 12 reconcilers + manager + leader election (+108)
6. `02ea4f3` binary-cont3: single-binary wire — **Charter §5 fix** — full
   kubectl surface + mock-CRI pod lifecycle; duplicate JSON parser
   reconciled in `1bf9b27` (+219)
7. `c233b51` MQTT 5.0 clean-room broker (v5 codec, QoS 0/1/2 router,
   TCP/TLS/WS runtime) replaces the stub (+87)

## Day 2 — charter fixes + real transports + long tail

**Charter §5 (single binary):** `cave-home-binary` now declares every pillar
crate as a real dependency (landed with binary-cont3). Stale "deferred
Phase 1b" claims retired (`ac7aeb8`): the runtime half is real
(`server::run`, signal-driven graceful shutdown, supervised lifecycle) and
its three parity-manifest entries moved `[[unmapped]] → [[mapped]]`.

**Real-transport / integration merges** (each test-gated):
hue CLIP v2 REST+SSE (`7021e93`), freeathome fhapi v1 REST+WS, flannel
netlink datapath (one `WatchEvent.prev_value` API-drift fix), coredns live
DNS server, kubelet CRI gRPC + WS streaming, traefik real reverse-proxy,
HA-core foundation, jarvis voice+LLM pipeline, secrets-encryption PQC
(ML-KEM-768 + AES-256-GCM), tracker.
`unifi` and `tesla` turned out to be already contained in the cont3 lineage.

**Long tail** (branches the audit called "finished but unmerged"):
tdd-audit records, jarvis family policy, esphome native-API codec, matter
WindowCovering + verified v1.3.0.0 provenance, zigbee 5 ZCL clusters, kine
etcd Txn RPC, flannel neigh/dualstack, kubelet cgroup-v2/GC/containerd
snapshotter, hue v2 grouped_light/motion/button controllers.
Superseded duplicates dropped instead of merged: core-honest entity registry
(ha-core-foundation's is fuller), nvr-honest zone filtering (already on
main), coredns scaffold (diff-empty).

**Parity index (`d81f4fd`):** all 14 null-honest crates now measured —
`adr_justified_ratio` counted from gap entries, `honest_ratio` computed via
`fill/(fill+(1-fill)*(1-adr))`; mobile/pool/wearable honestly 0.00. Four
crates merged this sweep (freeathome/jarvis/tracker/unifi) got their first
manifests. 69 crates, 0 nulls.

**K3s port mandate:** ADR-034 records the divergence (behavioural
reimplementation on main vs ADR-004's line-by-line mandate) + a
module-by-module convergence plan (kine first). `cave-home-orchestration`
`port-method` now references it. A verbatim rewrite did not fit 48h and
doing it silently would have been worse than the gap.

## Charter compliance status

- §5 single binary: **fixed** (real `[dependencies]`, in-process boot).
- K3s line-by-line: **divergence recorded + convergence program** (ADR-034).
- TDD strict: preserved — all merged branches carry red→green pairs; the one
  red-left test found (ServiceLbMetrics) was closed with its existing green
  impl commit, not weakened.
- Honest measurement: parity index has zero self-reported nulls; stale
  "Phase 1b deferred" claims deleted where the code is real.
- Apache-2.0 / clean-room: MQTT broker and esphome merges are clean-room;
  matter provenance corrected to the verified v1.3.0.0 tag.

## Remaining blockers / follow-ups

1. `claude/cave-home-k3s-scheduler-cm-2026-06-07` (kept, local+origin):
   NodeAffinity Score plugin + a richer ReplicaSet reconcile written against
   the *older* framework API — needs a real port to the current
   ScoreExtensions design, not a mechanical merge.
2. ADR-034 convergence program: first target `cave-home-kine-rs`.
3. mobile / pool / wearable: deferred scaffolds, honestly 0.00.
4. `cargo clippy --workspace --lib`: 0 errors; pre-existing warnings remain
   in untouched crates (cli 134, calendar 28, …) — cosmetic backlog.
5. ADR-034 is autopilot-accepted; founder ratification pending.
