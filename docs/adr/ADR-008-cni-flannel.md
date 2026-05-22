# ADR-008 — CNI: flannel line-by-line Rust port

## Status

**Accepted** — 2026-05-14, finalised by Burak Tartan (founder).

Created: 2026-05-14
Supersedes: —
Superseded by: —

## Context

ADR-004 picked **K3s line-by-line Rust port** as the cave-home
orchestration substrate. K3s needs a CNI (Container Network
Interface) plugin to provide pod networking. The previous Phase 1
crate scaffold left this as a generic `cave-home-cni-rs`
placeholder. That placeholder needs to commit to a specific
upstream so the Phase 1 port work can start.

Constraints that hold:

- **ADR-004 / Charter §5.** The chosen CNI must port cleanly into
  the unified Rust binary; no sidecar daemon.
- **ADR-002 / Charter §6.1.** Apache-2.0 or MIT upstream is
  required (line-by-line under Apache-2.0); copyleft CNI
  upstreams would force the clean-room path.
- **ADR-003.** Linux 7.1+ baseline. eBPF / io_uring available; we
  do not need to avoid modern kernel features.
- **ADR-007 / Charter §6.3.** The UI must never surface "CNI",
  "VXLAN", "flannel"; the user sees status like "Eklentilerim
  konuşuyor / sessiz". CNI choice is invisible to the headline
  persona.
- **Cave Runtime independence (Charter §5.1).** Cave Runtime
  ports Cilium; cave-home **does not consume** any Cave Runtime
  crate. Whichever CNI we pick must be reimplemented scratch in
  cave-home's tree (no path / git dep).

The home cluster scale is small (1–N nodes, typically 2–3 in
practice) — there is no need for a datacentre-grade L7 policy /
service mesh on day one.

## Decision

**cave-home-cni-flannel** — a line-by-line Rust port of
[`flannel-io/flannel`](https://github.com/flannel-io/flannel)
(Apache-2.0). Lands in `crates/cave-home-cni-flannel/` and ships
as part of Orchestration Phase 1 (parallel to M2 Zigbee in
ROADMAP.md).

Cilium is **deferred**. If cave-home's audience later needs L7
network policy, Hubble-class observability, or service-mesh
features, a follow-on ADR (informally ADR-008b) will introduce
`cave-home-cni-cilium` as an opt-in alternative. Cave Runtime's
Cilium port remains code-isolated from cave-home per Charter
§5.1; the cave-home Cilium port, if/when it lands, will be
scratch-reimplemented.

### Naming convention note

ADR-004 set the sub-crate suffix convention as `cave-home-X-rs`,
where `X` is the K3s-vendored component. **Concrete-upstream**
sub-crates (where cave-home pins a specific named upstream
rather than wrapping a K3s-vendored generic component) **drop
the `-rs` suffix and append the upstream name instead**:

- `cave-home-kubelet-rs` — generic kubelet from K3s-vendored
  kubernetes.
- `cave-home-cni-flannel` — concrete flannel CNI from
  `flannel-io/flannel`. (Future siblings: `cave-home-cni-cilium`
  if/when it lands.)

This extends rather than violates ADR-004's convention; both
forms remain unambiguous against Cave Runtime's `cave-cni-*`
crates.

## Consequences

### Accepted gains

- **Small upstream.** flannel is roughly **5K LOC Go** — portable
  on a tractable timeline as part of Orchestration Phase 1
  (parallel to M2 Zigbee, months 3–4).
- **K3s default.** flannel is K3s's default CNI; using it
  preserves the K3s-vendored test patterns and reduces
  surprise when cross-referencing upstream issues.
- **Apache-2.0 upstream.** Line-by-line port allowed under
  ADR-002 with no clean-room overhead.
- **VXLAN datapath is sufficient for home-cluster scale.** 1–N
  nodes on a LAN, no datacentre-grade throughput requirements.

### Accepted costs

- **Network policy is L4-only.** flannel does not implement
  Kubernetes NetworkPolicy at L7. For the v0.1 audience this is
  not a constraint (single household, single trust boundary),
  but it caps the policy surface available to add-ons.
- **No eBPF datapath.** flannel's VXLAN encapsulation runs in
  the kernel datapath but is not eBPF-accelerated. Performance
  is acceptable for home-cluster scale; if a user lands on
  cave-home with 10+ nodes and complex add-on traffic, Cilium
  becomes the answer (deferred ADR).
- **No service mesh / Hubble observability** out of the box.
  cave-home observability piggybacks on the regular metrics
  surface, not on a CNI-provided one.
- **Sub-project against the user's mental model is hidden.**
  Per ADR-007 / Charter §6.3, the user never sees "flannel",
  "CNI", or "VXLAN"; status is reported as "Eklentilerim
  konuşuyor / sessiz". This is by design and not a cost — it
  is recorded here so it stays visible to engineers reading the
  ADR.

## Alternatives considered

### (a) flannel *(chosen)*

- **Pro:** K3s default, ~5K LOC, Apache-2.0, simple VXLAN.
- **Con:** L4-only policy; no eBPF datapath.
- **Chosen.**

### (b) Cilium

- **Pro:** eBPF datapath, L7 network policy, Hubble observability,
  service-mesh features.
- **Con:** Enormous port surface (orders of magnitude larger than
  flannel). Overkill for 1–N-node home clusters. Engineering
  cost not justified by audience need at v0.1.
- **Deferred.** May become `cave-home-cni-cilium` via a future
  ADR. Cave Runtime's Cilium port is **not** consumed.

### (c) WireGuard + bridge

- **Pro:** Minimal datapath, encrypted-by-default.
- **Con:** No turn-key dynamic pod-to-pod routing primitives;
  cave-home would have to write extra controller logic to make
  it work like a real CNI. Net engineering cost not lower than
  flannel.
- **Rejected.**

### (d) Calico

- **Pro:** Mature, eBPF and BGP datapaths, broad enterprise
  adoption.
- **Con:** Enterprise-oriented; BGP is a poor fit for a single-
  household LAN; eBPF-mode benefits overlap with (b) Cilium.
- **Rejected.**

## Notes

This ADR locks the Phase 1 CNI choice. The
`cave-home-cni-flannel` crate (renamed from the earlier
`cave-home-cni-rs` placeholder) is the home for the port work.
Orchestration Phase 1 in ROADMAP M2 now reads:
*containerd + kubelet + **cave-home-cni-flannel** + kube-proxy*.
