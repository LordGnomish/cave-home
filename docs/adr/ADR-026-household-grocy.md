# ADR-026 — Household management (Grocy port)

## Status

**Accepted** — 2026-05-15, founder wholesale approval (Charter v6
expansion).

## Context

Grocy is the OSS household-management tool: food / medicine
inventory, chores, batteries (literal batteries — when did you
last change the smoke-detector battery?), shopping list, recipes.
HA users routinely run Grocy alongside HA; ADR-026 brings it
inside cave-home as a §3.2 pillar.

## Decision

`cave-home-household` — line-by-line port of Grocy
(`grocy/grocy`, MIT).

- Inventory (food, batteries, medicine).
- Chores + recurring task scheduling.
- Shopping list (synced to Mobile app per ADR-006).
- Battery tracker (ties into device-class entities: when did
  the door-sensor's battery get replaced? → automation can
  notify when a sensor's last-replacement crosses a threshold).

Port method: **line-by-line** (MIT).

## Consequences

### Accepted gains
- "Coffee is low" / "smoke-detector battery is 5 years old" /
  "kid's chore for Saturday" all live in one Portal alongside
  the automation rules that drive them.
- Battery-replacement tracker ties cleanly into the Zigbee /
  Z-Wave low-battery sensors — a single notification surface.

### Accepted costs
- Grocy is a relatively large surface; the line-by-line port
  is a multi-week effort.
- Image upload (recipe photos) needs filesystem-backed storage
  on the primary hub; ADR-005 deployment topology assumed
  ample storage.

### Charter §6.3 / ADR-007 compliance
UI says "Alışveriş listesi", "Bu hafta yapılacaklar", "Pil
değiştirme zamanı" — never "Grocy API", "stock entry ID".

## Alternatives considered

- (a) Defer Household to community add-on. Rejected per
  founder v6 wholesale approval.
- (b) Subset of Grocy (inventory only). Rejected — Grocy's
  value is the integration of inventory + chores + shopping,
  not any single piece.

## Notes

[ASSUMPTION: Grocy's PHP runtime is reimplemented in Rust per
the line-by-line rule; the port is *behavioural*, not literal-
PHP-translation. The cave-home crate exposes the same data
model and API surface, written in idiomatic Rust.]
