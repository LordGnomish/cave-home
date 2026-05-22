# ADR-031 — Wearable / sleep (deferred placeholder)

## Status

**Accepted** — 2026-05-15, founder wholesale approval (Charter v6
expansion). **Implementation deferred to M11+**; this ADR
authorises the workspace placeholder only.

## Context

Smart-bed and sleep-system integrations (Eight Sleep, Sleep
Number, Tempur-Pedic Smart Bed) sit alongside the wellness
pillar (ADR-025) but are a distinct hardware class: an actuator
(the bed itself), not just a sensor. The audience overlap with
Charter §2 persona-1 is real but small.

Daytime wearable integration (Apple Watch, Garmin, Whoop, Oura)
is already covered by ADR-025 (Wellness); ADR-031 is
specifically the **sleep-system actuator** + dedicated sleep
wearables niche.

## Decision

`cave-home-wearable` — **scaffold only**. An empty placeholder
crate lands in this commit. Implementation deferred to M11+.

Implementation will be line-by-line ports of:
- HA `eight_sleep` integration — Apache-2.0
- HA `sleep_number` integration — Apache-2.0
- Sleep-tracking integrations beyond what ADR-025 covers
  (Withings Sleep Analyzer is already in ADR-025).

Port method **when implemented**: line-by-line (all permissive).

## Consequences

### Accepted gains
- Workspace shape is set; the deferred work has a slot.
- Sleep-driven automations ("if Burak's sleep score < 60,
  delay the morning alarm by 30 min") have a target crate to
  hook into.

### Accepted costs
- Eight Sleep / Sleep Number both depend on vendor cloud APIs;
  Charter §9 boundary preserved at cave-home, user-side
  trade-off visible.
- Scaffold-without-implementation tax (one more crate to
  Cargo-build in CI for nothing) until M11+.

### Charter §6.3 / ADR-007 compliance
*Not applicable at scaffold stage.* Vocabulary added when the
crate ships: "Uyku puanı", "Yatak ısıt", "Akşam alarm hazır".

## Alternatives considered

- (a) Combine with ADR-025 (Wellness). Considered — sleep
  *trackers* are in ADR-025. Sleep *systems* (the bed
  actuator) are distinct hardware, so a separate crate keeps
  the engineering surfaces clean.
- (b) Drop entirely. Rejected per founder v6 wholesale
  approval.

## Notes

[ASSUMPTION: Sleep-system actuator support is treated as a
distinct pillar from Wellness because the *actuator* class
(temperature control, position control) is engineering-
different from the *sensor* class. If founder treats them as
one pillar, ADR-031 can be merged into ADR-025 and
`cave-home-wearable` folded into `cave-home-wellness`.]
