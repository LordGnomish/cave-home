# ADR-015 — Cover / garage / awning

## Status

**Accepted** — 2026-05-15, founder wholesale approval (Charter v6
expansion).

## Context

Garage doors, motorised blinds / shutters, awnings, gates. HA's
`cover` entity domain already abstracts these; the upstream
sources cave-home ports from are:

- **OpenGarage** (Apache-2.0) — open-source garage controller.
- **ESPHome** `cover` components — MIT (line-by-line OK; mixed-
  licence ESPHome is handled by the existing ESPHome adapter).
- **Somfy RTS** protocol (proprietary spec, partially
  reverse-engineered) — **clean-room** because no permissive
  reference implementation exists.

KNX-bound covers reach cave-home via `cave-home-knx` (ADR-011).
Zigbee / Z-Wave covers reach via the existing protocol pillars.

## Decision

`cave-home-cover` — **hybrid** port:

- OpenGarage HA integration: **line-by-line** (Apache-2.0).
- ESPHome cover bindings: **line-by-line** (MIT).
- Somfy RTS protocol: **clean-room** from public RF spec /
  Wireshark dissections / public reverse-engineering write-ups.

## Consequences

### Accepted gains
- Garage doors + motorised blinds + Somfy awnings covered in
  one crate.
- Cleanly composes with automation engine for sun-tracking
  blinds, presence-based garage close, etc.

### Accepted costs
- Somfy RTS clean-room means contributor recusal for the
  sub-module.
- Somfy RF requires an RFXtrx-class transceiver dongle —
  hardware-floor add to the §8 hardware list for users
  wanting Somfy support.

### Charter §6.3 / ADR-007 compliance
UI says "Garaj kapısı aç", "Salon panjur indir", "Tente kapat" —
never "Somfy RTS rolling code", "OpenGarage REST endpoint".

## Alternatives considered

- (a) Defer Somfy; ship OpenGarage + ESPHome only. Rejected —
  Somfy is too common in European homes to leave unsupported.
- (b) Bundle all cover protocols (incl. Zigbee/Z-Wave) here.
  Rejected — those live in the existing protocol pillars; this
  crate handles only the upstreams not covered there.

## Notes

[ASSUMPTION: Somfy RTS reverse-engineered protocol material in
public domain (e.g. various GitHub repos under permissive
licences) is treated as **clean-room reference** — contributors
should not read those repos either, just the protocol-level
write-ups. Conservative interpretation of Charter §6.1.]
