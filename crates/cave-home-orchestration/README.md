# cave-home-orchestration

**Status:** Umbrella crate. ADR-004 Accepted 2026-05-14 — cave-home's
orchestration layer is a **line-by-line Rust port of
[`k3s-io/k3s`](https://github.com/k3s-io/k3s)** (Apache-2.0),
implemented in this crate and its sub-crates, all linked into the
single cave-home unified binary (Charter §5).

See [`../../docs/adr/ADR-004-orchestration-layer.md`](../../docs/adr/ADR-004-orchestration-layer.md)
for the decision record, the rejected alternatives, and the phase
plan.

## Sub-crates (Phase 1–4)

Each sub-crate sits at `crates/cave-home-X-rs/` (flat workspace,
not nested). The `-rs` suffix distinguishes the cave-home K3s-
derived port from Cave Runtime's parallel-named crates, which port
vanilla `kubernetes/kubernetes` (see Charter §5.1 — **no code shared**
between the two trees).

| Crate                                 | Upstream                              | Phase |
| ------------------------------------- | ------------------------------------- | ----- |
| `cave-home-containerd-rs`             | containerd/containerd                 | 1     |
| `cave-home-kubelet-rs`                | kubernetes/kubernetes (K3s-vendored)  | 1     |
| `cave-home-kube-proxy-rs`             | kubernetes/kubernetes (K3s-vendored)  | 1     |
| `cave-home-cni-flannel`               | flannel-io/flannel                    | 1     |
| `cave-home-apiserver-rs`              | kubernetes/kubernetes (K3s-vendored)  | 2     |
| `cave-home-scheduler-rs`              | kubernetes/kubernetes (K3s-vendored)  | 2     |
| `cave-home-controller-manager-rs`     | kubernetes/kubernetes (K3s-vendored)  | 2     |
| `cave-home-kine-rs`                   | k3s-io/kine                           | 3     |
| `cave-home-helm-controller-rs`        | k3s-io/helm-controller                | 4     |
| `cave-home-klipper-lb-rs`             | rancher/klipper-lb                    | 4     |
| `cave-home-traefik-rs`                | traefik/traefik (optional ingress)    | 4     |

Phase mapping to ROADMAP milestones is parallel-track: Phase 1 runs
alongside M2 (Zigbee), Phase 2 alongside M3 (automation engine port),
Phase 3 alongside M4 (Matter + Z-Wave), Phase 4 alongside M5
(Camera / Voice / Mobile). Each phase's "done" criterion is recorded
in the relevant milestone in `ROADMAP.md`.

## Contributor rules

- Sub-crates port directly from `k3s-io/k3s` and the upstreams K3s
  itself depends on — **not from vanilla `kubernetes/kubernetes`**.
  The derivation chain matters for licence cleanliness and for not
  accidentally inheriting Cave Runtime work.
- All upstreams listed above are Apache-2.0 or MIT, so the
  line-by-line port discipline (Charter §6) applies. No clean-room
  upstream in this crate family today.
- The unified-binary mandate (Charter §5) is non-negotiable for
  orchestration; do not introduce sidecars or sub-processes.
- If during Phase 1 or 2 the scope cost looks unmanageable, file
  an issue rather than diverging from the port plan — an amending
  ADR can supersede ADR-004 with hybrid model (d).

## Why the `-rs` suffix?

Cave Runtime has crates named `cave-apiserver`, `cave-scheduler`,
`cave-kubelet`, etc., which port vanilla
[`kubernetes/kubernetes`](https://github.com/kubernetes/kubernetes).
cave-home has crates named `cave-home-apiserver-rs`,
`cave-home-scheduler-rs`, `cave-home-kubelet-rs`, etc., which port
[`k3s-io/k3s`](https://github.com/k3s-io/k3s) (and the
K3s-*vendored* kubernetes-internals).

The `cave-home-` prefix already disambiguates against `cave-`; the
`-rs` suffix is an extra contributor-facing marker that the
derivation chain is K3s, not vanilla K8s. If the suffix proves
redundant under PR-review experience, an amending ADR can drop it.
