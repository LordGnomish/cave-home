# ADR-028 — TV / display integration

## Status

**Accepted** — 2026-05-15, founder wholesale approval (Charter v6
expansion).

## Context

LG webOS TVs, Samsung Tizen TVs, Android TV (Chromecast), and
wall-tablet panel modes are the four common surfaces. HA core
already integrates LG / Samsung; wall-tablet mode in cave-home's
Portal is a new first-party concept that overlaps the
**WallPanel** Android pattern.

## Decision

`cave-home-display` — line-by-line port of TV-integration HA
entries + a first-party wall-panel mode for the cave-home
Portal:

- **HA `webostv` integration** — Apache-2.0
- **HA `samsungtv` integration** — Apache-2.0
- **HA `androidtv` / Cast integrations** — Apache-2.0
- **Wall-panel mode** — first-party Rust, served by the
  existing `cave-home-portal` crate. Tablet-friendly layout
  with motion-triggered wake, ambient brightness, room
  context. Conceptually similar to the WallPanel Android app
  but rendered by the cave-home Portal.

Port method: **line-by-line** for HA integrations; wall-panel
is first-party.

## Consequences

### Accepted gains
- "Çocuk odasında TV'yi söndür" voice command works for
  LG / Samsung / Android TV with one entity surface.
- Wall-tablet kitchen / hallway dashboards become a
  first-class deployment story.

### Accepted costs
- Vendor TV APIs drift (LG webOS rotation of auth model;
  Samsung's WoWLAN quirks). Per-vendor sub-modules.
- Wall-panel mode adds device-class entities for tablet
  hardware (motion sensor, brightness sensor); resource floor
  on the hardware ADR (ADR-032 reference hardware) shifts up
  for users running wall-panel mode.

### Charter §6.3 / ADR-007 compliance
UI says "Salon TV", "TV'yi yatak odasına bağla", "Tablet
duvar modu" — never "WebOS pointer service", "Tizen MAC
auth".

## Alternatives considered

- (a) Defer wall-panel mode. Rejected — the kitchen tablet
  use case is a §2 persona-1 ask.
- (b) WallPanel Android app integration. Rejected — WallPanel
  is unmaintained; first-party Portal wall-mode is the
  forward path.

## Notes

[ASSUMPTION: Wall-panel mode is a Portal *render mode*, not a
separate crate. cave-home-display covers TV vendor
integrations; cave-home-portal carries the wall-panel render
mode. Both ship as part of this commit's workspace.]
