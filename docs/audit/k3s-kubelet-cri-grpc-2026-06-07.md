# cave-home-kubelet-rs — containerd CRI gRPC client (2026-06-07)

Port of the **CRI v1 gRPC client** into `cave-home-kubelet-rs`, addressing the
"transport/IO is 100% unbuilt" blocker called out in
[`k3s-ground-truth-2026-06-07.md`](k3s-ground-truth-2026-06-07.md) §4.2: the
crate had a `CriClient` trait + in-memory mock, but nothing that actually
talked gRPC to containerd.

Branch: `feature/kubelet-cri-grpc` (isolated worktree). Local commits only, no
push, no merge — per the multi-writer-race instructions.

---

## 1. What was built

The real CRI client lives behind a **non-default `remote-cri` cargo feature**.
Default builds are unchanged: the decision core stays std/tokio with no gRPC
stack. With `--features remote-cri`:

| Layer | File | What |
|---|---|---|
| Wire contract | `proto/api.proto` | Verbatim CRI v1 from `kubernetes/cri-api@kubernetes-1.30.0`, gogo annotations removed (wire-identical) |
| Codegen | `build.rs` | `tonic-prost-build` → `src/cri/remote/proto` (only when feature on) |
| Marshalling | `src/cri/remote/conv.rs` | `From` impls: `cri::types` ⇆ generated proto (states mapped by *meaning*, not ordinal) |
| Errors | `src/cri/remote/error.rs` | `tonic::Status`/transport → `CriError` (NOT_FOUND stays typed) |
| Client | `src/cri/remote/client.rs` | `RemoteCriClient` impl `CriClient` over tonic; `connect_uds`/`connect_tcp`/`from_channel` |
| Streaming | `src/cri/remote/streaming.rs` | `exec`/`attach`/`port_forward` URL negotiation |

**RuntimeService coverage:** Version, Run/Stop/Remove/Status/List PodSandbox,
Create/Start/Stop/Remove/Status/List Container, Exec/Attach/PortForward
(URL negotiation). **ImageService:** PullImage, ImageStatus.

The port target is `k8s.io/kubernetes/pkg/kubelet/cri/remote/`
(`remote_runtime.go` + `remote_image.go` + `utils.go`).

### Why a feature flag (architecture decision)

Every K3s decision-core crate here (kubelet, containerd, apiserver, kine) is
deliberately dependency-free; the audit names the gRPC transport as the
deferred "Phase 1b" layer. Forcing tonic/prost onto the pure core would break
that architecture, while *not* building the client fails the task. The feature
flag reconciles both: the gRPC stack is opt-in, the decision core is untouched
by default (verified: `cargo test -p cave-home-kubelet-rs` with no feature =
87 tests, unchanged), and the transport is real and tested when enabled.

---

## 2. Test results

```
cargo test -p cave-home-kubelet-rs --features remote-cri
  => 193 passed; 0 failed; 1 ignored
cargo test -p cave-home-kubelet-rs            (default, no feature)
  =>  87 passed; 0 failed; 0 ignored   (decision core unchanged)
```

New CRI-remote tests (24 passing + 1 ignored):

| Test file | Cases | Coverage |
|---|---|---|
| `cri_remote_conv_test.rs` | 12 | native ⇆ proto, enum-by-meaning, filters |
| `cri_remote_error_test.rs` | 3 | gRPC Status → CriError |
| `cri_remote_client_test.rs` | 6 | client over a **real Unix-socket** mock CRI server, incl. **end-to-end** RunPodSandbox→CreateContainer→StartContainer→ContainerStatus→ListContainers |
| `cri_remote_streaming_test.rs` | 3 | Exec/Attach/PortForward URL negotiation |
| `cri_remote_containerd_test.rs` | 1 (`#[ignore]`) | full bring-up against a **real containerd** socket |

The mock server (`tests/common/mod.rs`) is a stateful in-process CRI runtime
served over a genuine UDS via `tonic::transport::Server` — so the client tests
exercise real protobuf-over-HTTP/2 round-trips, not an in-memory shortcut.

### Real containerd (acceptance criterion)

`cri_remote_containerd_test::real_containerd_pod_bringup` performs
Version → RunPodSandbox → PullImage → CreateContainer → StartContainer →
ContainerStatus → teardown against a live socket. It is `#[ignore]` and
env-gated (`CAVE_CRI_CONTAINERD_SOCK`, default
`/run/containerd/containerd.sock`).

**Not executed here:** this host is macOS with no containerd (`which containerd`
→ not found; no socket at `/run/containerd/containerd.sock`). The test is wired
to run unchanged on a Linux node with containerd:

```
cargo test -p cave-home-kubelet-rs --features remote-cri \
    --test cri_remote_containerd_test -- --ignored --nocapture
```

---

## 3. LOC ratio report

Upstream port-target (`pkg/kubelet/cri/remote/`, kubernetes release-1.30, code
lines excluding comments/blanks):

| Go file | code LOC |
|---|---|
| remote_runtime.go | 662 |
| remote_image.go | 177 |
| utils.go | 56 |
| **total** | **895** |

cave-home Rust port (hand-written, `src/cri/remote/`):

| Rust file | LOC |
|---|---|
| client.rs | 357 |
| conv.rs | 345 |
| streaming.rs | 54 |
| error.rs | 30 |
| mod.rs | 36 |
| **total** | **822** |

**LOC ratio ≈ 822 / 895 = 0.92.** Caveat (honest): this is *not* full-RPC
parity. The Rust client covers the ~18 RPCs the kubelet decision core drives
(sandbox + container lifecycle + image pull/status + streaming-URL); upstream
`remote_runtime.go` implements all ~30 RuntimeService RPCs. The near-1.0 ratio
reflects that the covered methods are ported at comparable density (and that
`conv.rs` absorbs marshalling upstream spreads across `kuberuntime`), **not**
that every RPC is done. Generated wire code (`runtime.v1.rs`, 5,706 LOC) is the
analogue of upstream `api.pb.go` and is excluded from the hand-written count.

Supporting code: vendored proto 1,919 LOC; tests 1,237 LOC; `build.rs` 23 LOC.

---

## 4. TDD compliance

Strict RED→GREEN, separate commits (`bec7a9a..HEAD`):

```
6d8f255 build(kubelet): add CRI v1 proto + tonic codegen behind remote-cri feature
d223416 test(kubelet): add failing tests for CRI native<->proto conversions   [RED]
0837787 feat(kubelet): implement CRI native<->proto conversions               [GREEN]
7253ff9 test(kubelet): add failing tests for gRPC Status -> CriError mapping   [RED]
03df824 feat(kubelet): map gRPC Status/transport errors to CriError            [GREEN]
51e7835 test(kubelet): add failing integration tests for RemoteCriClient ...   [RED]
3ce4631 feat(kubelet): implement RemoteCriClient gRPC transport for containerd [GREEN]
ae4c695 test(kubelet): add failing tests for Exec/Attach/PortForward URL ...   [RED]
3bf08f3 feat(kubelet): negotiate Exec/Attach/PortForward streaming URLs        [GREEN]
f50f8f8 test(kubelet): add ignored real-containerd CRI bring-up e2e
3fc1edc docs(kubelet): record remote-cri gRPC client in parity manifest
```

Every `test(...)` RED was confirmed failing (compile error / assertion) before
its `feat(...)` GREEN. 4 clean RED→GREEN pairs + the build scaffold (proto
codegen — not a behaviour unit) + the ignored real-containerd test.

---

## 5. 4-track mandate — honest disposition

- **Backend:** done (the gRPC client + transport).
- **Observability:** legitimately applicable — kubelet instruments CRI calls
  upstream (`pkg/kubelet/metrics`: `runtime_operations_total`,
  `runtime_operations_duration_seconds`, `_errors_total`). A real
  per-operation latency/error recorder wrapping `RemoteCriClient` is the
  correct next increment. **Deferred** (see §6), not faked.
- **cavectl + Portal:** intentionally **not** built. `cave-home-kubelet-rs` is
  hidden infrastructure (Charter §6.3, ADR-007) — no user-facing strings, no
  i18n. A CLI subcommand or Portal card for "the kubelet's containerd gRPC
  client" would be a stub with no grandma-facing value, i.e. a KIRMIZI
  honesty violation. This matches the established stance for every other K3s
  decision-core crate (kine/apiserver/containerd carry no CLI/Portal surface).
  Operators drive containerd via the runtime, not via a cave-home UI.

---

## 6. Deferred (phase-1b) — how to continue

1. **CRI byte-streaming dialer** — the SPDY/WebSocket client that dials the
   URL returned by Exec/Attach/PortForward and moves stdin/stdout/stderr. The
   negotiation half is done; this is the larger second half.
2. **Remaining RuntimeService RPCs** — ContainerStats/ListContainerStats,
   PodSandboxStats/List, UpdateRuntimeConfig, Status, CheckpointContainer,
   GetContainerEvents (server-stream), List*Metrics, RuntimeConfig,
   UpdateContainerResources, ReopenContainerLog. Generated stubs already exist
   in `src/cri/remote/proto`; these are thin adapters. They likely want a
   richer trait than the Phase-1 `CriClient` (which only models the decision
   core's surface).
3. **Remaining ImageService RPCs** — ListImages, RemoveImage, ImageFsInfo.
4. **Observability** — a `MeteredCriClient` decorator recording op
   count/latency/errors per method (the upstream metric set above).
5. **Wiring** — nothing instantiates `RemoteCriClient` yet; it is constructed
   but not yet handed to the `PodWorker`/`Kubelet` from a node bring-up path.
   That node-agent assembly is the integration step (still 0 across all K3s
   crates per the audit).

### Notes for the next session

- tonic **0.14.x** split codegen: runtime needs `tonic` + `tonic-prost`; build
  needs `tonic-prost-build`. Generated module is `runtime.v1` via
  `tonic::include_proto!`.
- UDS connect uses `Endpoint::connect_with_connector` + `tower::service_fn` +
  `hyper_util::rt::TokioIo` + `tokio::net::UnixStream` — works on tonic 0.14.
- **fmt gotcha:** `cargo fmt -p cave-home-kubelet-rs` reformats the *whole*
  crate and sweeps unrelated baseline files (the crate is not fmt-clean at
  baseline). Use `rustfmt --edition 2024 <file>` on new files only.
- **clippy:** keep `--lib -D warnings` clean for new code; `--all-targets` is
  not enforced for this crate (pre-existing test-target violations). Generated
  proto is `#[allow]`-wrapped in `src/cri/remote/mod.rs`.
- `proto/api.proto` removed only the 8 gogo lines; provenance recorded in its
  header. `protoc` (system, 35.0) is required at build time when the feature
  is on; `tonic-prost-build` drives it.
