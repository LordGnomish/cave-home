# ADR-034 — K3s port method: recorded divergence + line-by-line convergence plan

## Status

**Accepted** — 2026-06-16, autopilot sweep (pre-approved scope), pending
founder ratification.

Created: 2026-06-16
Amends: ADR-004
Superseded by: —

## Context

ADR-004 mandates a **line-by-line Rust port of `k3s-io/k3s`** for the
orchestration layer. What actually got built — and now lives on `main`,
green under `cargo test --workspace` — is different in method:

- `cave-home-orchestration` declares
  `port-method = "behavioural reimplementation (spec/contract-based; not
  verbatim line-by-line)"` in `Cargo.toml [package.metadata.cave-home]`,
  and its module docs say the same. Its sub-modules (bootstrap, bringup,
  component, config, role, shutdown, `local_path_provisioner`,
  `metrics_server`, `secrets_encryption`) are contract-based
  reimplementations of the documented K3s/Kubernetes behaviour.
- The K3s pillar sub-crates (`cave-home-kine-rs`, `cave-home-apiserver-rs`,
  `cave-home-scheduler-rs`, `cave-home-controller-manager-rs`,
  `cave-home-kubelet-rs`, `cave-home-cni-flannel`, `cave-home-coredns-rs`,
  `cave-home-traefik-rs`, `cave-home-klipper-lb-rs`) mix methods: some
  modules are ported against pinned upstream sources (e.g. flannel's
  datapath against `pkg/backend@d47fd8e`, local-path-provisioner against
  v0.0.36), most are spec/contract-based.

This is a **charter/ADR compliance gap, not a code-quality gap**: the 2026-06
audit and this sweep verified the behaviour end-to-end (kubectl 1.36 against
the in-process apiserver, pods running through the mock CRI, 5,569 workspace
tests green). Rewriting the working layer verbatim from Go is a multi-week
program that does not fit the 48-hour sweep, and doing it silently would
violate the honest-measurement rule harder than the method gap itself.

## Decision

1. **Record the divergence instead of hiding it.** Every orchestration-layer
   crate keeps an honest `port-method` in both `Cargo.toml` metadata and
   `parity.manifest.toml`; "line-by-line" may only be claimed where a pinned
   upstream SHA + file mapping exists (the fabricated-provenance sweep rule).
2. **Line-by-line convergence is a phased program, not a blocker.** The
   mandate stands as the *target state*. Convergence proceeds
   module-by-module: each round picks one subsystem, pins the upstream
   K3s/Kubernetes source file(s), ports them verbatim-with-Rust-idioms, and
   flips that module's `port-method`. Priority order: kine → apiserver
   handlers → scheduler framework → controller reconcilers → kubelet →
   networking (flannel/coredns/traefik/servicelb).
3. **The single-binary invariant (Charter §5) is unaffected** — it is already
   satisfied on `main` (`cave-home-binary` links every pillar in-process).

## Hand-off log (explicit, per sweep mandate)

- What exists: behaviour-verified contract-based orchestration layer, all on
  `main` as of 2026-06-16 (sweep commits `f1a231a..` onward), 0 test failures.
- What is owed: the module-by-module verbatim convergence above, tracked by
  flipping each crate's `port-method` + parity manifest as it lands.
- First convergence target: `cave-home-kine-rs` (smallest upstream surface,
  already has the real backend transport; upstream `k3s-io/kine` pin exists
  in its manifest).

## Consequences

- The parity index stays honest: `port-method` fields describe what the code
  is today; ADR-004's target is reachable incrementally without a rewrite
  freeze.
- Audits should flag as violations only crates claiming line-by-line without
  pinned provenance — not crates honestly declaring behavioural
  reimplementation with this ADR as justification.
