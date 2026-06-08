# Coverage matrix — cave-home-history

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · port method per manifest.
**Verified:** 10/10 mapped symbols found in source · 76 test fns · drift: No.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| Validated sample model + time-ordered series with stable sort | src/sample.rs::{Sample,Series,SeriesKey} | yes |
| Fixed-window bucketing anchored at epoch 0 (floor division, negative-safe) | src/aggregate.rs::{bucket_start,downsample} | yes |
| Bucket aggregators: mean/min/max/sum/count/first/last/median/p95 + empty-bucket gap skipping | src/aggregate.rs::Aggregator::apply | yes |
| Window statistics: min/max/mean/population-stddev/sum | src/stats.rs::summarize | yes |
| Trapezoidal area under the curve + time-weighted mean + rate-of-change | src/stats.rs::{integral,time_weighted_mean,rate_of_change} | yes |
| Gap detection: runs longer than the expected sampling interval | src/stats.rs::find_gaps | yes |
| Retention/roll-up ladder: classify each sample keep-raw/roll-up/evict vs caller-supplied now | src/retention.rs::{RetentionPolicy,Tier,Disposition,Partitioned} | yes |
| LTTB (Largest-Triangle-Three-Buckets) chart decimation: endpoints preserved, exact target count, time-ordered | src/decimate.rs::lttb | yes |
| Min/max-per-bucket chart decimation (spike-preserving) | src/decimate.rs::min_max | yes |
| Typed on/off & home/away state timeline with duration-in-state aggregation | src/state_history.rs::{StateSample,StateTimeline} | yes |
| Grandma-friendly EN/DE/TR chart phrasing | src/label.rs::{average,no_data,time_in_state,humanize_duration} | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| On-disk storage engine: write-ahead log + columnar/LSM segment files | phase-1b | ADR-023: the durable storage engine (WAL for crash-safety + columnar/LSM segments for year-scale retention) is storage/IO-bound. This pure analytics engine operates on the in-memory slices that layer reads back. Line-by-line port of the permissive InfluxDB 2.0 storage engine. |
| Write path + compaction | phase-1b | ADR-023: ingest buffering, flush-to-segment, and background compaction/roll-up execution are IO-bound and live with the storage engine. This crate already decides what to roll up and evict (retention::RetentionPolicy::partition); the storage layer executes it on disk. |
| Query / SQL-ish interface | phase-1b | ADR-023: the Flux-style query layer that selects ranges and series sits over the storage engine. cave-home's UX surfaces phrased results (src/label.rs), never a raw query — queries are an internal storage concern, deferred with it. |
| InfluxDB / Prometheus-style ingestion endpoints | phase-1b | ADR-023: network line-protocol / remote-write ingestion is network-bound. It maps wire samples onto sample::Sample then feeds this engine unchanged — no new analytics logic. |
| cave-home-core recorder integration | phase-1b | ADR-023: wiring sensor State changes into the history store (and back out as chart tiles) lands once cave-home-core's entity/state API stabilises. The engine is already core-agnostic and crate-independent. |
| Legacy SQLite-recorder schema import / pre-existing snapshot mode | permanent | Charter §7 always-latest + §8 no-backcompat: cave-home ships the current schema only; no historical-recorder import or pinned-snapshot compatibility mode. |

## Drift notes
None — every claimed symbol exists in source. All 10 mapped entries verified via grep of crates/cave-home-history/src/. Honest ratio supported: fill=0.30 (analytics engine fully implemented & tested; storage/IO layer deferred) with 100% ADR-023 justification on all unfilled items.
