# ADR-004 — Orchestration layer: K3s line-by-line Rust port

## Status

**Accepted** — 2026-05-14, finalised by Burak Tartan (founder).

Created: 2026-05-14
Supersedes: —
Superseded by: —

## Context

The founder confirmed in the previous round that cave-home needs a
**K3s-style orchestration layer** for third-party add-ons (HACS-style
community extensions, user-supplied workloads). The previous Draft
of this ADR enumerated four candidate shapes: (a) embed K3s, (b)
line-by-line Rust port, (c) Helm-deploy onto external K3s, (d)
hybrid (single-binary core + external orchestrator for add-ons).

This ADR settles the architectural shape.

Constraints that hold:

- **Charter §5 (single unified Rust binary).** Non-negotiable for
  the core. The carve-out paragraph allowed an orchestration layer
  to sit outside the core boundary *if needed*; this ADR decides
  whether to take that exit.
- **ADR-002 (Apache-2.0 + clean-room for copyleft).** K3s itself is
  Apache-2.0, so a line-by-line port is legally available; the
  dependency chain (containerd, runc, CNI plugins) is largely
  Apache-2.0 / MIT.
- **ADR-003 (Linux 7.1+, cgroup v2 mandatory, modern systemd).** Any
  orchestrator we adopt must run on this baseline without a
  legacy-cgroup fallback.

## Decision

**Line-by-line Rust port of `k3s-io/k3s` upstream**, implemented
in the `cave-home-orchestration` umbrella crate and its
sub-crates. The unified Rust binary mandate (Charter §5) is
**fully preserved**: core hub + orchestration layer compile into
**one** binary.

The carve-out in Charter §5 is therefore **not taken**: the
orchestration layer sits *inside* the unified binary, not
outside it. Third-party add-on containers (HACS-style) run on
this native, in-process orchestration layer.

### Why this shape

- **K3s is Apache-2.0.** Line-by-line port is legally available
  per ADR-002.
- **K3s is already "single-binary K8s" by design.** Porting K3s
  matches cave-home's unified-binary architecture far more
  naturally than porting vanilla Kubernetes would.
- **One binary preserves the cave-home UX promise.** "One
  install, one upgrade, one backup" is the headline; embedding a
  second binary (option a) or deploying onto an external
  cluster (option c) would erode it.
- **HACS-class third-party add-ons need a real container
  surface.** That requires *some* orchestrator. Doing it natively
  is more disciplined than wiring a sidecar.

### Implementation phases

Sequenced on top of the existing milestones in `ROADMAP.md`. The
orchestration port runs as a **parallel track** starting in M2 —
not a milestone-blocking serial step — so the smart-home pillars
do not stall behind it.

- **Phase 1 (parallel to M2 Zigbee, months 3–4).**
  Container runtime + node-side primitives.
  Crates: `cave-home-containerd-rs`, `cave-home-kubelet-rs`,
  `cave-home-kube-proxy-rs`, `cave-home-cni-flannel`
  (CNI choice locked by **ADR-008**: flannel line-by-line port).
- **Phase 2 (parallel to M3 automation engine, months 5–6).**
  Cluster control plane.
  Crates: `cave-home-apiserver-rs`, `cave-home-scheduler-rs`,
  `cave-home-controller-manager-rs`. Ports K3s's vendored
  Kubernetes wrappers, not vanilla `kubernetes/kubernetes`.
- **Phase 3 (parallel to M4 Matter + Z-Wave, months 7–9).**
  K3s's tea-tray trick — **kine**, the SQLite/Postgres-backed
  etcd-replacement that is the secret to K3s's single-binary
  story.
  Crate: `cave-home-kine-rs`.
- **Phase 4 (parallel to M5 NVR + Voice + Mobile, months 10–12).**
  K3s-spec ancillary components.
  Crates: `cave-home-helm-controller-rs`, `cave-home-klipper-lb-rs`,
  `cave-home-traefik-rs` (optional ingress; if rejected, ingress
  remains a TBD), plus a future `local-path-provisioner` crate.

### Naming convention

Sub-crates use the `cave-home-X-rs` suffix to disambiguate from
Cave Runtime's parallel-named crates (`cave-apiserver`,
`cave-scheduler`, `cave-kubelet`, etc.). The `-rs` suffix is a
contributor-facing marker that this is the **cave-home K3s-
derived port**, not the cave-runtime vanilla-K8s-derived port.

cave-home and Cave Runtime do **not** share code (Charter §5.1).
If the `-rs` suffix proves redundant in PR review (the
`cave-home-` prefix already disambiguates), an amending ADR can
drop it; until then, the suffix stays.

## Consequences

### Accepted gains

- **Architectural purity.** No sidecar binary; no second supervisor
  tree; Charter §5 honoured end-to-end.
- **Native Rust performance.** The K3s hot paths (kubelet, kube-
  proxy, kine) run without a Go GC tax.
- **One install, one upgrade, one backup** for the homeowner —
  the cave-home UX promise stays intact.
- **HACS-style add-on ecosystem unlocked** via a standard
  Kubernetes API surface (since cave-home's port preserves K3s's
  K8s-API compatibility).
- **K3s lives entirely under the hood.** Per ADR-007 (grandma-
  friendly UX, Charter §6.3), the Portal and Mobile app **never**
  surface K3s / pod / kubelet / etcd / kine vocabulary. The user
  sees "Ev Hub'ları" and "Eklentiler"; the underlying Kubernetes
  surface is reachable only from the Portal's **Developer view**
  toggle (off by default; absent from Mobile entirely). The
  implementation gain here is invisible to the user **on
  purpose** — that is the point of the mandate.

### Accepted costs

- **Enormous scope.** K3s itself is ~80K LOC of Go, but it
  vendors Kubernetes (~1.5M LOC). The Rust port will span
  years. ROADMAP gates Phases 1–4 across M2 → M5 to manage this;
  beyond M5 we likely need an M6+ "orchestration hardening"
  milestone (deferred to a later ADR).
- **Apparent duplication with Cave Runtime.** Cave Runtime ports
  vanilla `kubernetes/kubernetes`; cave-home ports `k3s-io/k3s`.
  This is **explicit duplication accepted** under Charter §5.1
  ("no crate reuse"). The derivation chains differ (K3s adds a
  significant layer over vanilla K8s — kine, klipper-lb, helm-
  controller, batteries-included defaults); the projects evolve
  independently and serve different audiences. No code is
  shared; even where Rust crates with similar names exist on
  both sides, **`cave-` (Cave Runtime) and `cave-home-` (cave-
  home) prefixes are non-interchangeable**.
- **Cave Runtime work does not accelerate cave-home work** and
  vice versa. Contributors moving between the two projects
  carry mental models, not code.

### Concrete follow-ups

1. `cave-home-orchestration` is upgraded from placeholder to
   **umbrella crate** that re-exports the sub-crates listed
   above; its README is rewritten to point at this ADR and the
   phase list.
2. The 11 Phase-1–4 sub-crates are scaffolded under `crates/`
   with full Cargo.toml metadata (upstream, port method,
   spec-source where applicable) but empty `lib.rs`.
3. ROADMAP M2.5 is renamed to "Orchestration Phase 1 (parallel
   to M2)"; M3, M4, M5 each grow a "parallel: Orchestration
   Phase N" bullet.
4. `docs/upstream/REFERENCES.md` gains an "Orchestration
   upstreams" sub-section listing K3s, containerd, vendored
   Kubernetes, kine, flannel, traefik, klipper-lb, and
   local-path-provisioner with port methods.
5. Charter §5's "orchestration carve-out (pending ADR-004)"
   paragraph is rewritten to point at this Accepted ADR.

## Alternatives rejected

The four candidates from the previous Draft are preserved here
for the record.

### (a) Embed — cave-home ships K3s upstream binary

**Rejected.** Erodes Charter §5: "one binary, one upgrade, one
backup" no longer holds when a second binary (and its own
dependency tree: containerd, runc, CNI plugins) is along for the
ride.

### (c) Deploy target — cave-home as Helm chart onto external K3s

**Rejected.** Structural breach of Charter §5 — cave-home stops
being a single unified binary at install time. Worst onboarding
UX for the headline homeowner persona (Charter §2).

### (d) Hybrid — single-binary core + external orchestrator for add-ons

**Rejected, but it was the runner-up.** (d) preserves Charter §5
for the core and was the carve-out paragraph's reason for
existing. (b) was preferred because **architectural purity won
over scope cost**: paying years of port work was deemed worth it
to avoid two surfaces, two upgrade paths, and two backup paths
for the lifetime of the project. If, during Phase 1 or 2, the
scope cost is judged unacceptable, this ADR can be superseded
in favour of (d).

## Open questions — resolved

1. **Add-on isolation.** Orchestration is **always on** — it
   compiles into every cave-home install. Isolation is via K3s
   pod boundary, not via a separate runtime.
2. **Existing-cluster reuse.** **Not supported** — cave-home
   runs its own K3s port. Homelabbers already running k0s /
   microk8s for other workloads either run cave-home as a
   separate stack or expose the cave-home K3s API to their
   other workloads. cave-home is self-sovereign by default.
3. **HACS source compatibility.** **Yes** — cave-home's K3s
   port implements the standard Kubernetes API; HACS-style
   add-on bundles shipped as Helm charts / K8s manifests are
   deployable. (Compatibility with HACS's *Home-Assistant-
   specific* surface is a separate ADR concern, not a K3s
   matter.)
4. **Cave Runtime alignment.** **None at code level.** Both
   projects expose a Kubernetes-API-compatible orchestrator,
   but they derive from different upstreams (k3s-io/k3s vs
   kubernetes/kubernetes) and share no code per Charter §5.1.
   Cross-project alignment is *protocol-level only* — a user
   could theoretically run an add-on on either cluster, but
   not via shared tooling.

## Notes

This ADR is Accepted, not a placeholder. The `cave-home-
orchestration` crate is no longer empty in spirit — it is now
the umbrella that the 11 phase-1–4 sub-crates link under.
Sub-crate scaffolds land in the same commit as this ADR's
acceptance.
