# ADR-022 — DNS + ad-blocking (AdGuard clean-room + Unbound)

## Status

**Accepted** — 2026-05-15, founder wholesale approval (Charter v6
expansion).

## Context

DNS-level ad-blocking + parental controls + per-device policy is
a §3.2 wave-1 pillar. AdGuard Home is the dominant OSS choice;
Pi-hole is the historical alternative (EUPL); Unbound is the
permissive recursive resolver that sits below either.

Both AdGuard Home and Pi-hole are copyleft (AdGuard GPL,
Pi-hole EUPL); Unbound is BSD.

## Decision

`cave-home-dns-adguard` — **clean-room** Rust reimplementation
of the AdGuard Home admin API + filter-rule engine, from the
public AdGuard documentation only. Contributors must NOT read
AdGuard source.

`cave-home-dns-unbound` — line-by-line port of Unbound (BSD)
+ HA `unbound` integration. This is the recursive-resolver
layer; `cave-home-dns-adguard` sits above it as the
filtering / policy layer.

[ASSUMPTION: Pi-hole is **deferred**. AdGuard Home covers the
same surface and the second clean-room track adds maintenance
load without proportional benefit. A future ADR may add Pi-hole
support if a meaningful audience needs it.]

Port methods:
- `cave-home-dns-adguard`: **clean-room** (Charter §6.1)
- `cave-home-dns-unbound`: line-by-line (BSD)

## Consequences

### Accepted gains
- Per-device DNS filtering (parental controls, ad-blocking,
  malware filtering) without a separate sidecar daemon.
- The cave-home LAN can resolve recursively, avoiding any
  third-party DNS provider — Charter §9-clean DNS path.

### Accepted costs
- AdGuard clean-room is one of the larger clean-room sub-ports.
  Contributors recused per Charter §6.1.
- DNS is **infrastructure-critical**; cave-home becomes a
  single point of failure for the home network's DNS unless
  the multi-node failover (Charter §5 / ADR-005) is configured.
  Recorded so users plan for it.

### Charter §6.3 / ADR-007 compliance
UI says "Reklamları engelle", "Çocuk filtresini aç",
"Telefon X için sınırlama" — never "DNS A record", "BIND
forwarder", "filter list ABP syntax".

## Alternatives considered

- (a) Pi-hole instead of AdGuard. Rejected — AdGuard's
  surface is broader (per-client rules, DHCP option) and the
  audience is larger.
- (b) Unbound only (skip filtering). Rejected — ad-blocking +
  parental controls is the headline §3.2 use case, not the
  recursive layer alone.

## Notes

[ASSUMPTION: AdGuard's public admin-API documentation
(github.com/AdguardTeam/AdGuardHome/wiki and the OpenAPI spec
they publish) is the sole permitted reference. Contributors do
not read the AdGuard Home Go source.]
