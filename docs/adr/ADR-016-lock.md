# ADR-016 — Smart-lock integration

## Status

**Accepted** — 2026-05-15, founder wholesale approval (Charter v6
expansion).

## Context

Front-door / interior smart locks (Nuki, SwitchBot Lock, August,
Yale, Aqara, ESPHome-flashed custom locks) are core Charter §2
persona-1 / 2 surface — "did the front door lock when I left?",
"let the cleaner in remotely". HA's `lock` entity domain already
abstracts these.

UniFi Access (Charter §3.1 / ADR-009) covers commercial / multi-
door installations; ADR-016 is about the residential lock
surface where UniFi Access is overkill.

## Decision

`cave-home-lock` — line-by-line port of the HA lock-domain
integrations:

- **Nuki** HA integration + Nuki Smart Lock REST API —
  Apache-2.0 / public API
- **SwitchBot** HA integration + SwitchBot Lock API — Apache-2.0
- **ESPHome** lock components — MIT (covered transitively by
  ESPHome adapter; `cave-home-lock` adds the lock-domain entity
  bindings)
- Zigbee / Z-Wave / Matter locks → handled via existing protocol
  pillars; this crate adds the lock-domain entity abstractions.

Port method: **line-by-line** (all permissive).

## Consequences

### Accepted gains
- All common residential lock vendors covered in one crate.
- Automations like "auto-lock at 22:00" or "unlock when family
  member arrives" compose cleanly with `cave-home-zigbee` /
  presence detection.

### Accepted costs
- Lock state is **safety-critical**; clean-room test fixtures
  are non-negotiable (Charter §6 golden rule). PRs that ship
  without integration tests against captured-trace fixtures
  are rejected.
- Vendor-cloud-dependent locks (Nuki Web, SwitchBot Cloud) mean
  some users will trade-off privacy for convenience; cave-home
  surfaces the trade-off in UI, does not silently route through
  the cloud.

### Charter §6.3 / ADR-007 compliance
UI says "Ön kapı kilitle", "Misafir için aç", "Çocuk eve geldi mi?" —
never "Nuki Web API token", "SwitchBot bot UUID".

## Alternatives considered

- (a) Defer locks to UniFi Access only. Rejected — UniFi Access
  is commercial-tier; residential locks need a separate path.
- (b) Single-vendor focus. Rejected — vendor lock-in is exactly
  what cave-home avoids.

## Notes

[ASSUMPTION: Nuki / SwitchBot public REST APIs are stable enough
to track upstream. If a vendor breaks API compatibility, the
crate tracks the breakage per Charter §7 always-latest mandate.]
