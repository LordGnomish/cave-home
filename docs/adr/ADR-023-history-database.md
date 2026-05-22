# ADR-023 — Time-series history database (sensor history)

## Status

**Accepted** — 2026-05-15, founder wholesale approval (Charter v6
expansion).

[ASSUMPTION: This ADR partially answers what was previously
listed as ADR-013 ("History database") in the older ADR-001
follow-on list. That entry is now obsolete; cave-home's sensor-
history layer is ADR-023.]

## Context

HA core has a `recorder` integration backed by SQLite by
default, with InfluxDB / TimescaleDB / VictoriaMetrics as
optional time-series back-ends. cave-home's pillar count has
grown enough (Solar, Camera, Voice, HVAC, Air-quality, ...) that
"5 years of every sensor every 5 seconds" is realistic; SQLite
alone is insufficient at that volume.

## Decision

`cave-home-history` — line-by-line port of relevant upstreams,
all permissive:

- **InfluxDB 2.0** (`influxdata/influxdb`) — MIT, line-by-line
  port of the storage engine + Flux query layer that cave-home
  uses as the *embedded* default for time-series.
- **TimescaleDB** (`timescale/timescaledb`) — Apache-2.0,
  available as an *opt-in* backend for users who already run
  PostgreSQL on the cluster.
- **VictoriaMetrics** (`VictoriaMetrics/VictoriaMetrics`) —
  Apache-2.0, alternative for users with very high cardinality
  needs.

Default backend: **InfluxDB 2.0 embedded** (matches HA's
default experience). Power users (Charter §2 persona 3–5) can
opt into TimescaleDB or VictoriaMetrics via Developer view.

SQLite remains the **automation-state** store (small, fast,
crash-safe); time-series sensor data goes to InfluxDB by
default.

Port method: **line-by-line** (all permissive).

## Consequences

### Accepted gains
- Energy + climate + air-quality history all queryable at year
  scale.
- Energy Dashboard (Charter §3) gets a proper time-series back-
  end instead of HA's SQLite recorder's known scaling pain.

### Accepted costs
- Storage footprint grows fast on the primary hub; ADR-005
  multi-node deployments may want a dedicated history node.
- Three back-ends means three test matrices; default-path
  (InfluxDB) gets the most coverage.

### Charter §6.3 / ADR-007 compliance
UI says "Geçen yıl üretim", "Bu hafta sıcaklık ortalaması",
"5 yıllık radon trendi" — never "Flux query", "InfluxDB
retention policy".

## Alternatives considered

- (a) SQLite recorder only (match HA default). Rejected —
  too small at cave-home's pillar count.
- (b) PostgreSQL + TimescaleDB only (skip embedded InfluxDB).
  Rejected — installs need a default that works on the
  smallest single-node deployment without a separate
  database.

## Notes

[ASSUMPTION: InfluxDB 2.0 MIT licence covers the storage
engine + Flux. Recent versions (3.x) moved to a different
licensing posture; cave-home pins to 2.x stable for the
line-by-line port. If 2.x is end-of-life'd, an amending ADR
will choose between VictoriaMetrics or a Rust-native
alternative.]
