# ADR-012 — HVAC / heat-pump / climate integrations

## Status

**Accepted** — 2026-05-15, founder wholesale approval (Charter v6
expansion). [ASSUMPTION: founder confirmed en-bloc; specific
upstream selection within category is contributor-discretion until
overridden.]

## Context

Heat pumps, air conditioners, and thermostats are the largest
single energy consumer in a typical Charter §2 persona-1 household,
and Burak's Iphofen residence (the primary user) runs a Viessmann
heat pump. HA core's climate domain already integrates Viessmann
(Open3EClient), Daikin, LG, Bosch, Mitsubishi, Samsung; generic
thermostats (Z-Wave / Zigbee / KNX-bound) are covered transitively
via existing pillars.

## Decision

`cave-home-hvac` — line-by-line port of the HA climate-domain
integrations relevant to residential heat pumps and ACs:

- **Open3EClient** (Viessmann ViCare) — Apache-2.0
- HA `daikin`, `lg_thinq`, `bosch_alarm`/HVAC, `mitsubishi_*`,
  `samsung_tv`-class climate integrations — Apache-2.0
- Generic thermostat entity (already in HA core) — Apache-2.0

Port method: **line-by-line** (all permissive). No clean-room
work in this crate today.

Plumbs into the cave-home automation engine + Energy Dashboard
(Charter §3 automation pillar) — heat-pump output is a major
input to the Solar Tier 1 surplus-management logic (ADR
post-Solar).

## Consequences

### Accepted gains
- Burak's Viessmann heat pump runs against cave-home day-one
  after this crate matures.
- Heat-pump load-shifting (run when solar surplus is high) ties
  into `cave-home-solar-evcc` cleanly.

### Accepted costs
- Vendor surface is broad (Viessmann + Daikin + LG + Bosch +
  Mitsubishi + Samsung); each has its own auth model and rate-
  limit quirks.
- Cloud-API integrations (LG ThinQ, Samsung) require vendor
  accounts — cave-home itself does not require a vendor account,
  but the user's device may; recorded so the limitation is
  visible.

### Charter §6.3 / ADR-007 compliance
UI says "Salon ısıtma", "Isı pompası modu", "Kazan sıcaklığı" —
never "Open3EClient endpoint", "ViCare API token".

## Alternatives considered

- (a) Defer HVAC to community add-ons. Rejected — too central
  to the headline persona's energy + comfort experience.
- (b) Single-vendor focus (Viessmann only). Rejected — locks
  cave-home to one user's house; the broader European audience
  needs Daikin / Bosch / Mitsubishi parity.

## Notes

[ASSUMPTION: cloud-API vendor accounts (LG ThinQ, Samsung) are
treated as user-side concerns, not cave-home-side privacy
violations. Charter §9 covers cave-home's own dependencies; users
opt into vendor accounts at their own risk.]
