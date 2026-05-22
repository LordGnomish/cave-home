# ADR-029 — Garden / outdoor (Husqvarna Automower + plants)

## Status

**Accepted** — 2026-05-15, founder wholesale approval (Charter v6
expansion).

## Context

Robot lawnmowers (Husqvarna Automower is dominant; Worx
Landroid, Stihl iMow secondary), soil-moisture sensors (Zigbee
+ MQTT + LoRa), garden lighting (covered by ADR-014), garden
irrigation (covered by ADR-013), and weather-station data feeds
form the "garden" pillar.

This ADR captures the lawnmower + soil-sensor surface that isn't
already covered elsewhere.

## Decision

`cave-home-garden` — line-by-line port of:

- HA `husqvarna_automower` integration — Apache-2.0
- HA `worx_landroid` integration — Apache-2.0 (community)
- MQTT-based soil-moisture / soil-temperature sensor bindings
  (transitive via `cave-home-zigbee` / `cave-home-mqtt`; this
  crate adds the garden-domain entity bindings).
- Weather-station feeds (Davis Vantage, Ecowitt, Netatmo
  Weather) — HA Apache-2.0 integrations, line-by-line.

Port method: **line-by-line** (all permissive).

## Consequences

### Accepted gains
- "Çim biçilmesin yağmur yağarken" automation composes cleanly
  with weather-feed + Automower entities.
- Soil-moisture-driven irrigation ties this crate to
  `cave-home-water` (ADR-013).

### Accepted costs
- Husqvarna Automower requires a vendor cloud account (Connect
  service); cave-home routes through the account but does not
  itself require one.
- Worx Landroid integration is community-maintained; upstream
  stability is per-vendor.

### Charter §6.3 / ADR-007 compliance
UI says "Çim biçme programı", "Yağmur sensörü tetikledi",
"Bahçe nem seviyesi" — never "Automower OAuth", "soil-moisture
ADC raw value".

## Alternatives considered

- (a) Defer Garden to a separate non-core dispatch. Rejected
  per founder v6 wholesale approval.
- (b) Lawnmower-only (skip weather + soil). Rejected — the
  three compose into the automation rules that justify the
  pillar.

## Notes

[ASSUMPTION: Husqvarna Connect cloud is the only viable path
for Automower (no local LAN API published as of 2026-05). If
Husqvarna publishes a local API, the crate is amended to
prefer it under Charter §9.]
