# ADR-013 — Water / irrigation / leak detection

## Status

**Accepted** — 2026-05-15, founder wholesale approval (Charter v6
expansion).

## Context

Outdoor irrigation (sprinklers / drip), indoor leak detection
(under-sink, washing machine, dishwasher), and smart water-meter
integration cover three failure modes the Charter §2 persona
notices the moment they go wrong: a flooded laundry room, a
neglected garden, a surprise water bill.

OpenSprinkler is the dominant OSS irrigation platform; Aqara /
Shelly Zigbee leak sensors integrate via the existing
`cave-home-zigbee` crate; smart water meters (S0 pulse,
Modbus-RTU, M-Bus) reach cave-home via HA's `modbus` / `mbus`
integrations.

## Decision

`cave-home-water` — line-by-line port of the OpenSprinkler HA
integration plus the relevant water-meter integrations.

- **OpenSprinkler** web app + HA integration — [ASSUMPTION:
  Apache-2.0 for the integration; OpenSprinkler firmware itself
  is GPLv3 but cave-home only ports the API client, not the
  firmware. If the firmware itself needs cave-home-side
  reimplementation, an amending ADR will treat it as clean-room.]
- HA `mbus`, `modbus` water-meter integrations — Apache-2.0
- Leak-detection logic — first-party Rust (no upstream port);
  consumes Zigbee leak sensors via `cave-home-zigbee`.

Port method: **line-by-line** for the API-client surface;
clean-room not required at this layer.

## Consequences

### Accepted gains
- Garden + indoor water covered with one crate.
- Leak detection ties into automations (notification, valve
  shutoff) via the existing automation engine.

### Accepted costs
- Vendor-specific water meter protocols (M-Bus dialect drift)
  may require per-vendor sub-modules over time.
- OpenSprinkler hardware support spans an old + new generation
  with different API surfaces.

### Charter §6.3 / ADR-007 compliance
UI says "Bahçe sulama programı", "Su kaçağı tespiti", "Aylık
su tüketimi" — never "OpenSprinkler API endpoint", "M-Bus slave
address".

## Alternatives considered

- (a) Skip irrigation; cover leak detection only. Rejected —
  Burak runs garden irrigation; the gap would be visible.
- (b) Defer water-meter integration. Rejected — energy and
  water dashboards both want utility-meter visibility for the
  same persona reasons.

## Notes

[ASSUMPTION: OpenSprinkler firmware GPL status is irrelevant to
cave-home as long as we only port the HA API-client integration
and the OpenSprinkler-web-app open-API surface. If a future need
arises to embed the firmware logic itself, that work becomes a
clean-room crate per Charter §6.1.]
