# ADR-017 — Robot vacuum (Valetudo port)

## Status

**Accepted** — 2026-05-15, founder wholesale approval (Charter v6
expansion).

## Context

Xiaomi / Roborock / Dreame / Viomi robot vacuums ship with
proprietary, cloud-locked stacks by default. **Valetudo** is the
OSS firmware-replacement that lets these vacuums run locally
with no vendor cloud — exactly the Charter §9 privacy posture.

## Decision

`cave-home-vacuum` — line-by-line port of Valetudo's HA
integration + Valetudo's REST/MQTT control surface (MIT).

- Valetudo upstream (`Hypfer/Valetudo`) — MIT, line-by-line.
- HA `xiaomi_miio` / vacuum-domain integrations — Apache-2.0,
  line-by-line.

Port method: **line-by-line** (all permissive).

cave-home does *not* attempt to re-flash vacuum firmware itself
— that remains a user-side operation. The crate talks to a
Valetudo-flashed vacuum over local network.

## Consequences

### Accepted gains
- Cloud-free vacuum operation matches Charter §9.
- Map + room-aware cleaning integrates with the automation
  engine (clean kitchen after dinner; skip baby room during
  nap).

### Accepted costs
- Vacuum must already be Valetudo-flashed; cave-home does not
  handle the rooting process.
- Hardware support is whatever Valetudo supports upstream.

### Charter §6.3 / ADR-007 compliance
UI says "Süpürgeyi salona gönder", "Mutfak temizle", "Şarja
dön" — never "Valetudo REST endpoint", "Xiaomi miio token".

## Alternatives considered

- (a) Direct Xiaomi cloud integration. Rejected — violates §9.
- (b) Defer vacuum until Matter Robot Vacuum is mature.
  Deferred separately; ADR-017 ships the Valetudo path now.

## Notes

[ASSUMPTION: Valetudo's "private cloud" posture is a stable
upstream design; if Valetudo upstream pivots, this ADR is
amended.]
