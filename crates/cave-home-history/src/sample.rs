//! The time-series sample model — the typed inputs the history engine reasons
//! about.
//!
//! A [`Sample`] is one numeric measurement at one point in time, tagged with a
//! [`SeriesKey`] that names what it belongs to (e.g. "living-room temperature").
//! Timestamps are caller-supplied epoch values: this engine has **no clock** —
//! the storage layer that feeds it (deferred to Phase 1b, see ADR-023) provides
//! both the samples and the "now" used by retention.
//!
//! A [`Series`] is an owned, time-ordered run of samples for one key. It is the
//! in-memory slice every other module ([`crate::aggregate`], [`crate::stats`],
//! [`crate::decimate`], …) operates on.

/// The timescale a [`Sample`] timestamp is expressed in.
///
/// The engine never assumes one or the other — bucketing and gap detection take
/// their window/interval in the *same* unit as the timestamps, so the caller is
/// free to use whichever the upstream store records. This enum exists only so a
/// caller can label a series for its own bookkeeping and so UX phrasing
/// ([`crate::label`]) can convert a duration to human words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeUnit {
    /// Timestamps are whole epoch **seconds**.
    Seconds,
    /// Timestamps are epoch **milliseconds**.
    Millis,
}

impl TimeUnit {
    /// How many of this unit make one second. Used by UX phrasing to turn a
    /// span of timestamps into "3 hours".
    #[must_use]
    pub const fn per_second(self) -> i64 {
        match self {
            Self::Seconds => 1,
            Self::Millis => 1000,
        }
    }
}

/// A stable name for one stream of measurements.
///
/// This is deliberately a plain owned string, not a protocol identifier: the
/// storage layer maps its own keys onto this, and nothing user-facing ever
/// shows it (Charter §6.3).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SeriesKey(String);

impl SeriesKey {
    /// Name a series.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// The raw name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for SeriesKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One measurement: a timestamp and a value.
///
/// Construction rejects a non-finite value up front so every downstream
/// aggregator and statistic can treat the value as a real number without
/// re-checking for `NaN`/infinity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    timestamp: i64,
    value: f64,
}

/// Why a [`Sample`] could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleError {
    /// The value was `NaN` or infinite.
    NotFinite,
}

impl core::fmt::Display for SampleError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotFinite => f.write_str("sample value is not finite"),
        }
    }
}

impl std::error::Error for SampleError {}

impl Sample {
    /// Construct a validated sample.
    ///
    /// # Errors
    /// Returns [`SampleError::NotFinite`] if `value` is `NaN` or infinite.
    pub fn new(timestamp: i64, value: f64) -> Result<Self, SampleError> {
        if !value.is_finite() {
            return Err(SampleError::NotFinite);
        }
        Ok(Self { timestamp, value })
    }

    /// The caller-supplied epoch timestamp.
    #[must_use]
    pub const fn timestamp(&self) -> i64 {
        self.timestamp
    }

    /// The measured value (always finite).
    #[must_use]
    pub const fn value(&self) -> f64 {
        self.value
    }
}

/// A time-ordered run of [`Sample`]s for one series.
///
/// Samples are kept sorted by timestamp (ascending). Use [`Series::sorted`] to
/// build from unordered input, or [`Series::from_sorted`] when the caller can
/// promise order. Equal timestamps keep their relative input order (a stable
/// sort), so a "last" aggregate is deterministic.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Series {
    samples: Vec<Sample>,
}

impl Series {
    /// An empty series.
    #[must_use]
    pub const fn empty() -> Self {
        Self { samples: Vec::new() }
    }

    /// Build a series from samples that are **already** ascending by timestamp.
    /// If they are not, downstream bucketing/gap results are unspecified; prefer
    /// [`Series::sorted`] when unsure.
    #[must_use]
    pub const fn from_sorted(samples: Vec<Sample>) -> Self {
        Self { samples }
    }

    /// Build a series from samples in any order, sorting them ascending by
    /// timestamp with a stable sort (ties keep input order).
    #[must_use]
    pub fn sorted(mut samples: Vec<Sample>) -> Self {
        samples.sort_by_key(Sample::timestamp);
        Self { samples }
    }

    /// The samples, in time order.
    #[must_use]
    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }

    /// Number of samples.
    #[must_use]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether the series has no samples.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// The timestamp of the earliest sample, if any.
    #[must_use]
    pub fn first_timestamp(&self) -> Option<i64> {
        self.samples.first().map(Sample::timestamp)
    }

    /// The timestamp of the latest sample, if any.
    #[must_use]
    pub fn last_timestamp(&self) -> Option<i64> {
        self.samples.last().map(Sample::timestamp)
    }

    /// The time span covered, latest minus earliest, or `0` for fewer than two
    /// samples.
    #[must_use]
    pub fn span(&self) -> i64 {
        match (self.first_timestamp(), self.last_timestamp()) {
            (Some(a), Some(b)) => b - a,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: i64, v: f64) -> Sample {
        Sample::new(t, v).expect("finite test sample")
    }

    #[test]
    fn rejects_non_finite_value() {
        assert_eq!(Sample::new(0, f64::NAN), Err(SampleError::NotFinite));
        assert_eq!(Sample::new(0, f64::INFINITY), Err(SampleError::NotFinite));
        assert_eq!(Sample::new(0, f64::NEG_INFINITY), Err(SampleError::NotFinite));
    }

    #[test]
    fn accepts_finite_including_negative_and_zero() {
        // Unlike a concentration, a time-series value may be negative
        // (temperature below zero, net power export, …).
        assert!(Sample::new(10, -5.0).is_ok());
        assert!(Sample::new(10, 0.0).is_ok());
        assert_eq!(s(10, 21.5).value(), 21.5);
        assert_eq!(s(10, 21.5).timestamp(), 10);
    }

    #[test]
    fn sorted_orders_by_timestamp_stably() {
        let series = Series::sorted(vec![s(30, 3.0), s(10, 1.0), s(20, 2.0)]);
        let ts: Vec<i64> = series.samples().iter().map(Sample::timestamp).collect();
        assert_eq!(ts, vec![10, 20, 30]);
    }

    #[test]
    fn equal_timestamps_keep_input_order() {
        // Two samples at t=10: "a" came first, so a "last" aggregate is "b".
        let series = Series::sorted(vec![s(10, 1.0), s(10, 2.0), s(5, 0.0)]);
        let vals: Vec<f64> = series.samples().iter().map(Sample::value).collect();
        assert_eq!(vals, vec![0.0, 1.0, 2.0]);
    }

    #[test]
    fn span_and_endpoints() {
        let series = Series::sorted(vec![s(100, 1.0), s(160, 2.0), s(220, 3.0)]);
        assert_eq!(series.first_timestamp(), Some(100));
        assert_eq!(series.last_timestamp(), Some(220));
        assert_eq!(series.span(), 120);
        assert_eq!(series.len(), 3);
        assert!(!series.is_empty());
    }

    #[test]
    fn empty_series_has_no_endpoints_and_zero_span() {
        let series = Series::empty();
        assert!(series.is_empty());
        assert_eq!(series.first_timestamp(), None);
        assert_eq!(series.last_timestamp(), None);
        assert_eq!(series.span(), 0);
    }

    #[test]
    fn series_key_roundtrips_and_displays() {
        let k = SeriesKey::new("living-room temperature");
        assert_eq!(k.as_str(), "living-room temperature");
        assert_eq!(k.to_string(), "living-room temperature");
    }

    #[test]
    fn time_unit_per_second() {
        assert_eq!(TimeUnit::Seconds.per_second(), 1);
        assert_eq!(TimeUnit::Millis.per_second(), 1000);
    }
}
