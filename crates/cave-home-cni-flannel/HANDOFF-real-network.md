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

## Progress log

- (in progress) Track 1 starting.
