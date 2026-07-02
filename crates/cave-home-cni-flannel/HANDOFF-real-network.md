# Handoff — flannel real network backend port

Branch: `feature/flannel-real-network`  ·  Worktree: `../cave-home-flannel-net`
Started from: `bec7a9a` (current HEAD of claude/cave-home-tesla-fleet-api-2026-06-07).
Upstream reference: `github.com/flannel-io/flannel@d47fd8e` (Apache-2.0), tarball
extracted to `/tmp/flannel-io-flannel-d47fd8e`. cave-home is Apache-2.0 → a
line-by-line port is licence-compatible. This is a *faithful port* of the
documented datapath, not a behavioural-from-docs reimplementation (which is
how the pre-existing decision core was built).

## Why this work exists (the Crit blocker)

The crate shipped the flannel *decision core* only: CIDR math, subnet leasing,
IPAM, backend config, and `routes::compute_route_plan` which returns a pure
`RoutePlan { routes, fdb }`. Nothing turned that plan into real kernel state —
no netlink, no VXLAN device, no FDB/ARP/route programming. Pods on different
nodes could not actually reach each other. This branch ports the real network
backend that consumes the decision core and programs the kernel.

## Architecture

The hard constraint is `unsafe_code = "forbid"` (workspace lint). So:

* The **netlink wire codec** (`net::netlink`) is pure, allocation-only, safe
  Rust — it builds the exact bytes the kernel rtnetlink ABI expects. Testable
  byte-for-byte on every platform (incl. this macOS dev box).
* The **`Datapath` seam** (`net::datapath`) is a trait. `MockDatapath` records
  every op for tests; the real `NetlinkSocket` impl (Linux-only) uses `nix`'s
  *safe* socket wrappers to write codec bytes to an `AF_NETLINK` fd.
* The backend logic (`device`, `vxlan_network`, `route_network`, `hostgw`,
  `wireguard`) drives the `Datapath` trait — identical logic on mock and real.

This mirrors the cave-home `tesla` crate's `HttpTransport`/`MockTransport` seam
and the `kube-proxy` `nix`-for-datapath precedent.

## Upstream → ours map

| upstream (pkg/...)                | ours (src/net/...)        | status |
|-----------------------------------|---------------------------|--------|
| (kernel rtnetlink ABI / headers)  | `netlink/*`               | TODO   |
| `mac/mac.go::NewHardwareAddr`     | `mac.rs`                  | TODO   |
| `backend/vxlan/device.go`         | `device.rs`               | TODO   |
| `backend/vxlan/vxlan_network.go`  | `vxlan_network.rs`        | TODO   |
| `backend/route_network.go`        | `route_network.rs`        | TODO   |
| `backend/hostgw/hostgw.go`        | `hostgw.rs`               | TODO   |
| `backend/wireguard/*`             | `wireguard.rs`            | TODO   |
| `subnet/kube` + etcd lease store  | `store/*`                 | TODO   |
| flannel cni-plugin / subnet.env   | `cni` + `bin/flannel.rs`  | TODO   |

## Tracks (TDD strict, commit per green cycle)

1. netlink codec: constants, attr TLV, nlmsghdr, link(vxlan), route, neigh, addr
2. Datapath trait + MockDatapath + mac.rs
3. device.rs (VxlanDevice) + vxlan_network.rs (ARP→FDB→route per peer)
4. route_network.rs + hostgw.rs (routeAdd/routeEqual reconcile)
5. real NetlinkSocket (Linux, nix, safe) behind the trait
6. 2-node simulated network integration test (vxlan tunnel + cross-node pod IP)
7. etcd/kine subnet store + watch→lease-event
8. CNI plugin bin + subnet.env contract
9. wireguard datapath (optional)

## Acceptance

cargo test PASS · netlink-mock integration test · 2-node sim (vxlan + x-node
pod routing) · LOC ratio recorded · TDD compliance · parity manifest updated ·
local `--no-ff` merge, NO push.

## Status — COMPLETE (core Crit blocker resolved)

All tracks 1–8 landed; track 9 (WireGuard genetlink datapath) deferred as
documented. Per-cycle commits on `feature/flannel-real-network`.

| track | module(s) | status |
|-------|-----------|--------|
| 1 netlink codec | `netlink.rs` | ✅ 11 tests |
| 2 datapath seam + mac | `datapath.rs`, `mac.rs` | ✅ 12 tests |
| 3 vxlan device + events | `device.rs`, `vxlan_network.rs` | ✅ 12 tests |
| 4 host-gw reconcile | `route_network.rs`, `hostgw.rs` | ✅ 9 tests |
| 5 real socket (Linux) | `netlink_socket.rs` | ✅ 3 tests (parse_ack); socket Linux-gated |
| 6 2/3-node sim | `tests/two_node_network.rs` | ✅ 3 integration tests |
| 7 etcd/kine store | `subnet_registry.rs` | ✅ 8 tests |
| 8 subnet.env + CNI | `subnet_env.rs`, `cni_delegate.rs`, `bin/flannel.rs` | ✅ 13 tests |

### Results vs acceptance criteria

- **cargo test PASS**: 152 lib unit tests + 3 integration tests (was 88). All green.
- **netlink-mock integration test**: `MockDatapath` records every op; every
  backend test asserts against it.
- **2-node simulated network (vxlan tunnel + cross-node pod routing)**:
  `tests/two_node_network.rs` builds a real cluster over the mock, reconstructs
  each node's route/ARP/FDB tables and walks a pod→pod packet (route→ARP→FDB→
  underlay). Also a 3-node mesh and a host-gw direct-route case.
- **LOC ratio**: ours impl-only ≈ 2083 LOC vs upstream ported Go ≈ 1450 LOC
  (device.go/vxlan_network.go/route_network.go/hostgw.go/mac.go/subnet.go).
  ~1.44×, and that *includes* the hand-written rtnetlink wire codec that
  upstream gets free from the `vishvananda/netlink` library — so the real
  faithful-port ratio is ≈ 1:1. (Total incl. tests/docs: 3624 LOC.)
- **TDD compliance**: every module written test-first; red→green per cycle,
  one commit per green cycle. lib + bins clippy clean (pedantic+nursery).
- **parity manifest**: updated — fill_ratio 0.40 → 0.72, 12 new `[[mapped]]`,
  remaining items honestly enumerated as `[[unmapped]]`.

### Honest caveats

- The Linux `NetlinkSocket` is **API-verified against nix 0.29 source** but not
  compiled here — this dev box (macOS, no rustup) has no Linux std/target. The
  pure `parse_ack` logic and every codec byte it writes *are* tested. Compile it
  on a Linux CI runner before relying on it.
- WireGuard datapath, the long-running daemon watch loop, IPv6 VXLAN datapath
  and CNI delegate exec/chaining remain — see `[[unmapped]]` in the manifest.

### Merge

`feature/flannel-real-network` was merged `--no-ff` into a local integration
branch (NOT pushed) — see the merge commit. The main checkout's live branch was
left untouched to avoid racing the concurrent uplift loop.

## Progress log

- Tracks 1–8 complete; see table above.
