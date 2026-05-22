# ADR-025 — Wellness / health integrations

## Status

**Accepted** — 2026-05-15, founder wholesale approval (Charter v6
expansion).

[ASSUMPTION: The earlier AskUserQuestion answer narrowed scope to
"smart-home + 3 ecosystems"; founder's subsequent v6 wholesale-
approval dispatch overrides that narrowing and brings Wellness
back in. This ADR records the full v6 scope. If founder intends
a tighter scope, this ADR is amended.]

## Context

Withings (sleep, scale, blood-pressure), Garmin (watches, bike
computers), Fitbit (watches, scales), Oura (rings), Apple Health
(via HomeKit), Whoop (subscription wearable). Integration is
read-only ("did Burak sleep well?", "is the family's activity
balance off?") and feeds the §3 automation engine ("dim lights
when sleep score < 60").

All major vendors offer cloud APIs; some (Withings) offer local
hubs.

## Decision

`cave-home-wellness` — line-by-line port of the HA wellness-
class integrations:

- HA `withings` integration — Apache-2.0
- HA `garmin_connect` integration — Apache-2.0
- HA `fitbit` integration — Apache-2.0
- HA `oura` integration — Apache-2.0
- Apple Health surfaced via HomeKit (existing cave-home HomeKit
  accessory bridge from `cave-home-portal` / Scrypted port).

Port method: **line-by-line** (all permissive). Cloud-API
integrations require vendor accounts on the user side; cave-
home itself stays account-free.

## Consequences

### Accepted gains
- Wellness data lives alongside home automation data, in the
  same time-series back-end (ADR-023). "Lower house
  temperature for better sleep" automations become trivial.
- Family-shared wellness dashboards (with role-based
  visibility per ADR-007 Family-role surface) are a single-
  pillar feature.

### Accepted costs
- Vendor cloud accounts are unavoidable for most wearables;
  Charter §9 is preserved at the cave-home boundary but the
  user-side trade-off is visible.
- API auth model varies (OAuth, app-secret, token rotation).
- This pillar sits at the boundary of cave-home's smart-home
  framing; users who treat health data as out-of-scope for a
  smart-home tool can simply leave the crate unwired.

### Charter §6.3 / ADR-007 compliance
UI says "Bu gece uyku puanı", "Bu hafta aktivite", "Sağlık
özetin" — never "Withings OAuth scope", "Garmin Connect API
token".

## Alternatives considered

- (a) Defer Wellness; treat it as out-of-scope. **Considered
  and rejected** per founder v6 wholesale approval (recorded
  as ASSUMPTION above).
- (b) Local-only wearables (Withings hub, ANT+ direct). Defer
  to a follow-on; cloud APIs are the audience-wide baseline.

## Notes

[ASSUMPTION: Wellness data is treated as Family-role-gated in
the Portal per ADR-007's role surface. Children / guests do not
see parents' sleep / weight by default. UX detail belongs in a
follow-on UX-spec doc.]
