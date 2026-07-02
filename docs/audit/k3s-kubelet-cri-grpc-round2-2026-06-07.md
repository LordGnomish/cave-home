# cave-home-kubelet-rs — CRI round 2: ImageService + streaming proxy (2026-06-07)

Continuation of [`k3s-kubelet-cri-grpc-2026-06-07.md`](k3s-kubelet-cri-grpc-2026-06-07.md).
Round 1 built the CRI v1 gRPC client scaffold (sandbox + container lifecycle +
image pull/status + Exec/Attach/PortForward **URL negotiation**). Round 2
closes the two largest CRI gaps that round 1 deferred:

1. The **ImageService** completion (`ListImages`, `RemoveImage`, `ImageFsInfo`).
2. The **streaming byte-transfer proxy** — the half that actually moves
   stdin/stdout/stderr/port bytes over the negotiated Exec/Attach/PortForward
   URL.

Branch: `feature/kubelet-cri-grpc` (same isolated worktree). Local commits
only, no push, no merge.

---

## 1. What was built

### 1a. ImageService completeness

`CriClient` gained `list_images(filter)`, `remove_image` (idempotent — a
runtime `NOT_FOUND` maps to `Ok`), and `image_fs_info`. A new native
`cri::types::FilesystemUsage` flattens the wire's nested
`FilesystemIdentifier` / `UInt64Value` wrappers. Wired through all three
`CriClient` impls (`RemoteCriClient`, `MockCriClient`, and the in-process mock
gRPC server, which now also honours the `ListImages` filter and returns
`ImageFsInfo` like a real runtime).

### 1b. WebSocket `v5.channel.k8s.io` streaming proxy

A dependency-free WebSocket stack under `src/cri/remote/ws/`:

| Layer | File | What |
|---|---|---|
| Handshake crypto | `ws/handshake.rs` | RFC 3174 SHA-1 + RFC 4648 base64 + RFC 6455 `Sec-WebSocket-Accept` (no new crates) |
| Frame codec | `ws/frame.rs` | RFC 6455 encode (client-masked) / decode (unmask, byte-accounted, incomplete-aware) — all 3 length classes, control frames, 16 MiB guard |
| Connection | `ws/conn.rs` | `WsConnection<S>`: role-aware client/server opening handshake + buffered framing; transparent Ping→Pong, Close→`Ok(None)` |
| Proxy | `ws/proxy.rs` | v5 channel demux (stdin0/stdout1/stderr2/error3/resize4/close255) + `run_exec`, `run_port_forward`, `dial(url)` |

Plus client glue: `RemoteCriClient::{exec_streamed, attach_streamed,
port_forward_streamed}` compose the full kubelet path —
**negotiate URL over gRPC → dial over WebSocket → bridge bytes**.

### Why WebSocket, not SPDY (the explicit round-2 ask was "SPDY proxy")

This is the one honesty call worth stating plainly. The task asked for a SPDY
proxy; round 2 ships a **WebSocket** proxy instead, for two hard reasons:

1. **Offline-toolchain constraint (decisive).** Interop-grade SPDY/3.1
   compresses its header blocks with zlib seeded by the fixed SPDY dictionary
   (`deflateSetDictionary`). No dictionary-capable zlib backend is buildable in
   this crate's frozen offline cache: `flate2`'s C backend needs `crc32fast` +
   `cc` + `pkg-config` (all uncached) and its `zlib-rs` backend needs an
   uncached `zlib-rs 0.6`; `miniz_oxide` (cached) exposes no preset-dictionary
   API. A from-scratch DEFLATE+dictionary codec is a multi-thousand-line
   undertaking out of scope for one round. A *no-dictionary* "SPDY" would not
   interoperate with real `moby/spdystream` — i.e. a stub, which the cave
   golden rule forbids.
2. **WebSocket is the correct forward transport for the pinned version.**
   Kubernetes **1.30** (the version `proto/api.proto` is copied from) ships the
   `v5.channel.k8s.io` WebSocket sub-protocol as a first-class CRI streaming
   transport, and SPDY is on the upstream deprecation path. RFC 6455 needs only
   SHA-1 + base64 for its handshake, both supplied locally — **zero new
   dependencies.**

The legacy SPDY/3.1 dialer stays recorded as deferred phase-1b in the parity
manifest. WebSocket is real, complete, and verified end-to-end below.

---

## 2. Test results

```
cargo test -p cave-home-kubelet-rs --features remote-cri
  => 219 passed; 0 failed; 1 ignored      (was 193 in round 1: +26)
cargo test -p cave-home-kubelet-rs            (default, no feature)
  => 172 passed; 0 failed; 0 ignored       (decision core unchanged)
cargo clippy -p cave-home-kubelet-rs --features remote-cri --lib
  => baseline (94 pre-existing pedantic/nursery warns; +0 net from round 2)
```

New round-2 tests (+26):

| Test | Cases | Coverage |
|---|---|---|
| `ws::handshake` (unit) | 3 | SHA-1/base64 known vectors + RFC 6455 §1.3 accept example |
| `ws::frame` (unit) | 8 | masked+unmasked roundtrips, 7/16/64-bit lengths, control frames, masking obscures payload, incomplete buffer, single-frame consume, reserved-opcode reject |
| `ws::conn` (unit) | 3 | duplex handshake + subprotocol negotiation + echo, 200 KB over a small pipe (chunked reads), transparent Ping→Pong then data |
| `ws::proxy` (unit) | 3 | channel_frame/split_channel codec |
| `cri_ws_streaming_test` (e2e, real TCP) | 3 | exec stdin→stdout/stderr + error-channel outcome, v5 stdin half-close on EOF, single-port forward with LE16 port header |
| `cri_remote_streaming_test` | +1 | **full path**: `exec_streamed` negotiates over gRPC then dials+bridges a real WS server |
| `cri_test` (mock) | +3 | list/remove/fs-info on `MockCriClient` |
| `cri_remote_client_test` (gRPC e2e) | +2 | list/remove + image-fs-info round-trips over the UDS gRPC server |

The streaming proxy is exercised against **two** real doubles: an in-process
WebSocket server over real TCP (`cri_ws_streaming_test`) and — for the headline
compose test — a gRPC mock that hands back a live WS URL so the negotiate→dial→
stream path runs in one test.

### Real containerd (acceptance criterion)

`cri_remote_containerd_test::real_containerd_pod_bringup` (`#[ignore]`,
env-gated `CAVE_CRI_CONTAINERD_SOCK`) was extended this round to also list the
pulled image, read `ImageFsInfo`, run a **streamed `echo` via `exec_streamed`**
(tolerant: logs-and-skips if the runtime's streaming server is SPDY-only), and
`RemoveImage` at teardown. **Not executed here** — this host is macOS with no
containerd. Run on a Linux node:

```
cargo test -p cave-home-kubelet-rs --features remote-cri \
    --test cri_remote_containerd_test -- --ignored --nocapture
```

---

## 3. LOC report

Hand-written round-2 Rust (implementation only, excluding `#[cfg(test)]`):

| File | LOC |
|---|---|
| `ws/proxy.rs` | 251 |
| `ws/conn.rs` | 240 |
| `ws/frame.rs` | 193 |
| `ws/handshake.rs` | 106 |
| `ws/mod.rs` | 34 |
| **ws subtotal** | **824** |
| ImageService additions (client/conv/mock/types) | ~95 |
| **round-2 total** | **~920** |

**Upstream analogue (honest framing).** The WebSocket transport's nearest Go
counterparts are `k8s.io/apimachinery/pkg/util/httpstream/wsstream` +
`k8s.io/client-go/tools/remotecommand/{websocket.go,v5.go,v4.go}` (~700 code
LOC), **plus** the RFC 6455 framing + handshake that Go imports from
`gorilla/websocket` (a whole external library, ~3k LOC) rather than writing
in-tree. A 1:1 ratio is therefore misleading: cave-home hand-writes the framing
layer Go vendors. Measured against the in-tree Go (`wsstream` + `remotecommand`
websocket), 824/~700 ≈ **1.18**, with the extra accounted for by the
hand-rolled RFC 6455 codec + SHA-1/base64. The ImageService additions track
`remote_image.go`'s `ListImages`/`RemoveImage`/`ImageFsInfo` (~70 Go LOC) at
~95 Rust LOC across three impls + conv.

Generated wire code is unchanged from round 1 (excluded).

---

## 4. TDD compliance

Strict RED→GREEN, separate commits (round-2 range, after `23e7721`):

```
feat(kubelet): complete CRI ImageService (list/remove/fs-info)        [test-first within commit]
test(kubelet): add failing tests for WS handshake crypto              [RED]
feat(kubelet): implement WS handshake crypto (SHA-1 + base64)         [GREEN]
test(kubelet): add failing tests for RFC6455 WS frame codec           [RED]
feat(kubelet): implement RFC6455 WS frame codec                       [GREEN]
test(kubelet): add failing tests for WS connection layer              [RED]
feat(kubelet): implement async WS connection (handshake + framing)    [GREEN]
test(kubelet): add failing tests for v5 channel streaming proxy       [RED]
feat(kubelet): implement v5 channel streaming proxy + client glue     [GREEN]
```

Four clean RED→GREEN pairs for the streaming stack (handshake, frame, conn,
proxy), each RED confirmed failing before its GREEN; ImageService landed
tests-before-impl within one commit.

---

## 5. 4-track mandate — unchanged disposition

Backend done; observability (a `MeteredCriClient` op-latency/error decorator)
still the correct deferred next increment; **no cavectl/Portal** — this is
hidden infrastructure (Charter §6.3, ADR-007), consistent with every other K3s
decision-core crate. A UI for "the kubelet's containerd streaming proxy" would
be a KIRMIZI honesty violation.

---

## 6. Deferred (phase-1b) — how to continue

1. **Legacy SPDY/3.1 dialer** — only needed for runtimes whose streaming server
   predates WebSocket support. Requires a dictionary-capable zlib (see §1b);
   revisit if/when the toolchain gains one, or vendor a DEFLATE codec.
2. **Dynamic terminal resize** — `run_exec` sends an initial channel-4 resize;
   a live resize channel (SIGWINCH→channel 4 mid-session) is the follow-on.
3. **Multi-port forward** — `run_port_forward` drives one local↔container port;
   the multi-stream layout (channels `2*i`/`2*i+1`) is the extension.
4. **wss/TLS dial** — `dial` handles `http`/`ws`; `https`/`wss` (rustls) is
   deferred, mirroring the round-1 `connect_tcp` TLS note.
5. **Remaining RuntimeService RPCs** + **observability** + **node-agent wiring**
   — unchanged from round 1 §6.

### Notes for the next session

- The WS stack is **zero-dependency** on purpose (SHA-1 + base64 hand-rolled).
  Don't "upgrade" it to `tokio-tungstenite` — that crate + its deps
  (`tungstenite`, `sha1`) are **not** in the offline cache.
- `WsConnection` is role-aware: clients mask, servers don't. `accept()` is
  public so the same type backs the in-process test servers.
- Masking keys are a non-cryptographic xorshift — fine for a trusted localhost
  CRI socket (no caching proxies), explicitly noted in `conn.rs`.
- Lib clippy must stay at baseline (94). Wire-codec cast/single-char lints are
  silenced with *targeted* `#[allow]` + justifying comments, not blanket.
