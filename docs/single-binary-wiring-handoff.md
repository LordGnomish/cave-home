# Single-binary wiring — handoff

**Branch:** `feature/single-binary-wire` (worktree `../cave-home-binary-integration`)
**Date:** 2026-06-07
**Charter:** §5 single-binary mandate — every cave-home crate compiles into one process.

## What this delivers

Before this work the K3s-class crates were each a standalone **pure decision
core** (no tokio, no HTTP, no TCP) and `cave-home-binary` only *described* a
bring-up plan — `main.rs` never started anything. This wires them into a single
binary that actually boots.

`cargo run --bin cave-home -- server` now:

1. seeds the local **Node** into an in-process `apiserver-rs::Registry` shared
   behind a `tokio::sync::Mutex`;
2. computes the dependency-ordered bring-up plan with the real
   `cave-home-orchestration` planner;
3. binds a **real TCP listener on `:6443`** and serves the apiserver read path
   over a hand-rolled std HTTP/1.1 codec;
4. spawns **one concurrent supervised task per control-plane component**, each
   driving its real decision core on an interval;
5. winds the components down in **reverse bring-up order** on SIGINT/SIGTERM.

### Modules added (all TDD: test → fail → impl → pass)

| File | What |
|------|------|
| `cave-home-binary/src/http.rs` | std-only HTTP/1.1 request parser + response writer (10 tests) |
| `cave-home-binary/src/apirest.rs` | HTTP → `Registry` verb mapping; `/api/v1/{nodes,pods}`, `/healthz`, `/version`, `/metrics` (10 tests) |
| `cave-home-binary/src/node.rs` | local Node self-registration object (5 tests) |
| `cave-home-binary/src/server.rs` | tokio runtime: bind, accept loop, supervisor, graceful shutdown (5 tests incl. real-socket + boot/shutdown) |
| `cave-home-binary/src/cli.rs` | `server` / `agent` / `etcd` commands (+6 tests) |
| `cave-home-binary/src/main.rs` | `Serve` dispatch → tokio runtime → `server::run` |
| `cave-home-cli/src/commands/get.rs` | `cavehomectl get nodes\|pods` over a std HTTP client (7 tests) |
| `cave-home-portal/src/cluster.rs` | Cluster/Workloads/Networking/Storage/Security developer pages (+ cards) (8 tests) |

### Concurrent components (real per-tick core calls)

`controller-manager` runs a genuine node-heartbeat **through the real
`reconcile::step` loop** (re-registers the Node if it disappears). `traefik`
builds+validates an ingress `DynamicConfig`; `klipper-lb`/servicelb runs
`controller::reconcile`; `scheduler`/`kubelet` observe the apiserver for
pending / locally-bound pods; `kine`/`apiserver` are the registry + accept loop.

## Acceptance — verified end to end

```
$ cave-home server --port 16443 --bind 127.0.0.1
cave-home: bring-up order: kine → apiserver → scheduler → controller-manager → cni → kubelet → kube-proxy → helm-controller → servicelb → traefik
cave-home: apiserver listening on 127.0.0.1:16443   # 10 components started concurrently

$ cavehomectl get nodes --server 127.0.0.1:16443
NAME
cave-home

$ cavehomectl get pods --server 127.0.0.1:16443
No pods found.                                       # empty list

# SIGTERM → reverse-order teardown → "home stopped cleanly", exit 0
```

- **Single binary size (release):** ~2 MB — well under the 200 MB budget.
- **Tests:** binary 108, cli 136+24, portal 66+ — all green.
- **Gate:** `cargo clippy -p cave-home-binary --lib` clean on the new files.

## Honesty boundaries — explicit follow-ups (no stubs, documented gaps)

These were **not** faked; each is a real next increment:

1. **Write verbs over the wire.** `apiserver-rs::json` has serialization but no
   parser, so the HTTP server is GET-only; non-GET on a resource returns `405`.
   The binary seeds its own objects in-process (Node). Next: a JSON request-body
   parser → POST/PUT/PATCH/DELETE.
2. **TLS on `:6443`.** Served as plain HTTP (no TLS crate is linked). Real K3s
   uses mTLS here.
3. **No CoreDNS crate exists** in the workspace — omitted from the bring-up set
   honestly (the task listed it, but there is no `cave-home-coredns-rs`).
4. **kube-proxy / helm-controller cores are not linked** into the binary
   (`Reconcile::AwaitingLink`); they are supervised placeholders. Add the crate
   deps + real ticks to activate.
5. **kubelet pod sync needs a CRI runtime** (containerd); with no container
   runtime present the kubelet task only *observes* assigned pods. Real
   `sync_pod` wiring is a follow-up.
6. **Live-state reconcile inputs.** scheduler/flannel/traefik/klipper drive
   their real cores but over minimal/own in-memory inputs, not yet over live
   apiserver state (that needs the registry-`Value` → typed-struct translation
   layer, which the apiserver JSON parser above unblocks).
7. **`etcd` role** currently maps to `PrimaryHub`; a datastore-only topology is
   a follow-up.
8. **Portal HTTP serving.** `cluster::operator_dashboard()` composes the pages
   at the view-model layer; the Portal is not yet served over a socket by this
   binary.

## Where to continue

The unblocking keystone is the **apiserver JSON request-body parser** (item 1):
it enables write verbs, which enables `cavehomectl apply`, which lets the
scheduler/kubelet loops reconcile real pods end-to-end (items 5–6).
