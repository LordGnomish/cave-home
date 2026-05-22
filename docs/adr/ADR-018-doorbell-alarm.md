# ADR-018 — Doorbell / intercom + alarm panel

## Status

**Accepted** — 2026-05-15, founder wholesale approval (Charter v6
expansion).

## Context

Two related but distinct surfaces:

1. **Doorbells / intercoms** — Reolink, DoorBird, Ring (RTSP-only
   path), and the UniFi Talk-class intercom already covered by
   ADR-009. Camera-side capture composes with the Frigate /
   UniFi Protect camera pillar.
2. **Alarm panels** — AlarmDecoder for legacy Honeywell / DSC
   wired panels, plus modern panel-class integrations (HA's
   `alarm_control_panel` entity domain).

The two share an entity model (sensor triggers a notification /
camera capture / siren activation) but live in different
upstreams. ADR-018 covers both with two crates.

## Decision

`cave-home-doorbell` — line-by-line port of:
- HA `reolink` integration — Apache-2.0
- HA `doorbird` integration — Apache-2.0
- Ring RTSP-only doorbell integration — Apache-2.0
- DoorBird HTTP/REST API client — public API

`cave-home-alarm` — line-by-line port of:
- HA `alarmdecoder` integration — Apache-2.0 (talks to
  AlarmDecoder USB / network adapters for Honeywell / DSC)
- HA `alarm_control_panel` entity-domain helpers — Apache-2.0
- Various per-panel HA integrations (Bosch, ELK-M1, etc.) —
  Apache-2.0

Port method: **line-by-line** (all permissive).

## Consequences

### Accepted gains
- Doorbell push to Mobile companion app (ADR-006) on press,
  with camera live view, becomes a native cave-home feature.
- Legacy wired alarm panels (common in older homes) get a
  cave-home arming surface without a vendor-cloud account.

### Accepted costs
- Ring is intentionally limited to RTSP-only because the full
  Ring stack is cloud-locked; no Ring cloud-account
  integration. Some Ring users will see a feature-degraded
  experience and choose another brand.
- AlarmDecoder hardware support drift over time — board
  generations matter.

### Charter §6.3 / ADR-007 compliance
UI says "Ön kapı zili çalındı", "Alarm kurulu", "Hareket
algılandı" — never "AlarmDecoder USB CDC", "Reolink HTTP API".

## Alternatives considered

- (a) Ring cloud integration. Rejected — privacy posture (§9).
- (b) Defer alarm panels to community add-ons. Rejected —
  safety-critical pillar; first-party support is non-
  negotiable.

## Notes

[ASSUMPTION: Charter §9 privacy posture for safety-critical
panels — cave-home back-end is the single trust boundary;
no third-party alarm-monitoring relay. Users who want
monitored-alarm-service can integrate at the §3.2 notify
pillar, not at the alarm pillar.]
