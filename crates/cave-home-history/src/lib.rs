//! `cave-home-history` — the time-series **history processing engine** for
//! cave-home (ADR-023).
//!
//! This crate is the analytics brain over the household's sensor history: it
//! downsamples raw samples into chart-ready buckets, computes window statistics
//! (including the time-weighted average a thermostat history should report),
//! decides retention/roll-up as data ages, decimates long series for plotting,
//! detects gaps where a sensor went quiet, tracks how long things were *on* or
//! *home*, and phrases all of it in grandma-friendly EN / DE / TR.
//!
//! # Scope (Phase 1 MVP — pure logic, std-only)
//!
//! Implemented, real and tested here. Everything operates on **in-memory
//! slices** with **caller-supplied timestamps and "now"** — there is no clock,
//! no network and no storage in this crate:
//!
//! - [`sample`] — the [`Sample`] / [`Series`] model and [`SeriesKey`].
//! - [`aggregate`] — fixed-window downsampling with mean / min / max / sum /
//!   count / first / last / median / p95, skipping empty buckets.
//! - [`stats`] — min/max/mean/stddev/sum, trapezoidal area under the curve, the
//!   time-weighted mean and rate-of-change, and gap detection.
//! - [`retention`] — a retention ladder that classifies each sample as
//!   keep-raw / roll-up / evict as a function of "now".
//! - [`decimate`] — LTTB and min/max-per-bucket decimation for charts.
//! - [`state_history`] — typed (on/off, home/away) state timelines with
//!   duration-in-state.
//! - [`label`] — localised chart phrasing (Charter §6.3, ADR-007).
//!
//! # Deferred to Phase 1b (see `parity.manifest.toml` `[[unmapped]]`, ADR-023)
//!
//! The **on-disk storage engine** is storage/IO-bound and is not in this crate:
//! the write-ahead log + columnar/LSM segment files, the write path and
//! compaction, a query interface, the InfluxDB/Prometheus-style ingestion
//! endpoints, and cave-home-core recorder integration. That layer *feeds* this
//! pure engine the slices it reasons over.
//!
//! # Example
//!
//! ```
//! use cave_home_history::{Sample, Series, Aggregator, downsample, summarize, Lang, average};
//!
//! // A handful of temperature readings (epoch seconds, °C).
//! let series = Series::sorted(vec![
//!     Sample::new(0, 20.0).unwrap(),
//!     Sample::new(30, 22.0).unwrap(),
//!     Sample::new(90, 21.0).unwrap(),
//! ]);
//!
//! // Downsample into 1-minute buckets, averaging each.
//! let perminute = downsample(&series, 60, Aggregator::Mean);
//! assert_eq!(perminute.len(), 2); // bucket [0,60) and [60,120)
//!
//! // Summarize the window and phrase it for the household.
//! let summary = summarize(&series).unwrap();
//! let caption = average(Lang::En, "today", summary.mean, "°");
//! assert_eq!(caption, "Average today: 21°");
//! ```

pub mod aggregate;
pub mod decimate;
pub mod label;
pub mod retention;
pub mod sample;
pub mod state_history;
pub mod stats;

pub use aggregate::{bucket_start, downsample, Aggregator};
pub use decimate::{lttb, min_max};
pub use label::{average, humanize_duration, no_data, time_in_state, time_in_state_units, Lang};
pub use retention::{Disposition, Partitioned, RetentionPolicy, Tier};
pub use sample::{Sample, SampleError, Series, SeriesKey, TimeUnit};
pub use state_history::{StateSample, StateTimeline};
pub use stats::{
    find_gaps, integral, rate_of_change, summarize, time_weighted_mean, Gap, Summary,
};
