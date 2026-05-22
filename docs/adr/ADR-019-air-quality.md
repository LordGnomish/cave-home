# ADR-019 — Air quality sensors

## Status

**Accepted** — 2026-05-15, founder wholesale approval (Charter v6
expansion).

## Context

CO₂, PM2.5/PM10, VOC, radon. Air-quality monitoring overlaps
HVAC (ventilation triggers), bedroom comfort (CO₂-based sleep
quality), and lung-health concerns. AirGradient, Awair, IKEA
Vindriktning (ESPHome-flashed) and Airthings (radon) cover the
typical sensor set; HA's `sensor` entity domain abstracts them.

## Decision

`cave-home-air-quality` — line-by-line port of relevant HA
integrations + first-party threshold logic for automation
triggers.

- **AirGradient** firmware + HA integration — ESPHome-class
  (MIT for the ESPHome bindings); design files CC-BY-SA which
  is documentation-licence-only, not a code-licence concern.
- **Awair** HA integration + public REST API — Apache-2.0.
- **IKEA Vindriktning** via ESPHome — covered by ESPHome adapter
  transitively; this crate adds the air-quality sensor-domain
  bindings.
- **Airthings** (radon) HA integration + public REST API —
  Apache-2.0.

Port method: **line-by-line** (all permissive).

## Consequences

### Accepted gains
- Bedroom CO₂ > 1000 ppm → automated ventilation; PM2.5 spike
  during cooking → kitchen fan auto-on. Native automation
  composition.
- Radon awareness for households in basements / on radon-prone
  ground.

### Accepted costs
- AirGradient is a build-your-own kit; supporting it well means
  documenting the assembly path elsewhere (out of scope for
  this crate).
- Awair / Airthings cloud APIs require vendor accounts on the
  user side; cave-home itself stays account-free.

### Charter §6.3 / ADR-007 compliance
UI says "Yatak odası havası tazelendiği", "Mutfakta PM2.5
yüksek", "Radon seviyesi normal" — never "AirGradient ESP MAC",
"Awair OAuth scope".

## Alternatives considered

- (a) Single-vendor focus (Awair only). Rejected — vendor
  diversity matters for budget-tier vs premium-tier households.
- (b) ESPHome-only (skip cloud APIs). Rejected — Airthings
  radon device has no ESPHome path; cloud-API integration is
  the only sovereign option for that sensor class.

## Notes

[ASSUMPTION: AirGradient CC-BY-SA on the hardware design files
does not contaminate cave-home's Apache-2.0 code tree — design
files are not code. If a future legal review disagrees, the
crate is amended.]
