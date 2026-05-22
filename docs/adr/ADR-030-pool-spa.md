# ADR-030 — Pool / spa (deferred placeholder)

## Status

**Accepted** — 2026-05-15, founder wholesale approval (Charter v6
expansion). **Implementation deferred to M11+**; this ADR
authorises the workspace placeholder only.

## Context

Pool + spa integrations (Hayward, Pentair, Jandy, Intex Spa)
are a Charter §3.2 wave-1 pillar in the v6 expansion, but the
audience is narrow (pool-owning households are a subset of the
§2 persona base). The combination of audience-narrowness and
vendor-API complexity (most pool controllers ship cloud-only
APIs) makes immediate implementation poor ROI compared to the
other §3.2 categories.

## Decision

`cave-home-pool` — **scaffold only**. An empty placeholder
crate lands in this commit. The crate's `src/lib.rs` carries a
doc-comment pointing at this ADR and at M11+ in the ROADMAP.

Implementation will be line-by-line ports of:
- HA `hayward_omnilogic` integration — Apache-2.0
- HA `pentair_intellicenter` integration — Apache-2.0
- Optionally local-LAN pool controllers (per-vendor)

Port method **when implemented**: line-by-line (all permissive).

## Consequences

### Accepted gains
- Workspace shape is set; Cargo.toml dependents (binary,
  CI matrix) account for the crate's existence from day one.
- No drift risk: the crate exists, the ADR exists, the
  ROADMAP slot exists. Nobody re-decides the question.

### Accepted costs
- Workspace member count grows by 1 with no shipping value
  until M11+.
- Pool-owning Charter §2 personas wait for M11+. Recorded so
  expectations are honest.

### Charter §6.3 / ADR-007 compliance
*Not applicable at scaffold stage.* Vocabulary added when the
crate ships: "Havuz sıcaklığı", "Filtre programı", "Spa modu".

## Alternatives considered

- (a) Skip pool entirely. Rejected per founder v6 wholesale
  approval — keep the pillar in scope, defer the work.
- (b) Ship pool in M10. Rejected — the wave-1 sequencing in
  ROADMAP M8–M10 already loads M10 with display + garden +
  household + calendar; pool is a stretch.

## Notes

[ASSUMPTION: Pool support is a real Charter §3.2 commitment,
not a "maybe-someday" backlog item. The deferred status means
**when M11 starts**, the crate is implemented, not
re-negotiated. If founder later decides pool is out of scope
entirely, this ADR is Superseded.]
