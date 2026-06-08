//! Window statistics and gap detection over a [`Series`].
//!
//! Everything here is pure arithmetic over an in-memory, time-ordered slice:
//! the descriptive summary ([`summarize`]), the time-weighted [`integral`]
//! (area under the curve, trapezoidal) and its derived [`mean_rate`], the
//! end-to-end [`rate_of_change`], and [`find_gaps`] for spotting where a sensor
//! went quiet. No clock, no storage — the storage layer (Phase 1b, ADR-023)
//! feeds the slice.

use crate::sample::{Sample, Series};

/// A descriptive summary of a window of samples.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    /// Number of samples summarized.
    pub count: usize,
    /// Smallest value.
    pub min: f64,
    /// Largest value.
    pub max: f64,
    /// Arithmetic mean.
    pub mean: f64,
    /// Population standard deviation (divides by `n`).
    pub stddev: f64,
    /// Sum of all values.
    pub sum: f64,
}

/// Summarize a window. Returns `None` for an empty series — there is nothing to
/// describe, and a fabricated zero would be a lie a chart could show.
#[must_use]
pub fn summarize(series: &Series) -> Option<Summary> {
    let samples = series.samples();
    let n = samples.len();
    if n == 0 {
        return None;
    }
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    let mut sum = 0.0;
    for s in samples {
        let v = s.value();
        min = min.min(v);
        max = max.max(v);
        sum += v;
    }
    let mean = sum / n as f64;
    let variance = samples
        .iter()
        .map(|s| {
            let d = s.value() - mean;
            d * d
        })
        .sum::<f64>()
        / n as f64;
    Some(Summary {
        count: n,
        min,
        max,
        mean,
        stddev: variance.sqrt(),
        sum,
    })
}

/// Trapezoidal integral (area under the curve) across the series, in
/// value·time units.
///
/// Each adjacent pair contributes `(v0 + v1) / 2 · (t1 - t0)`. With fewer than
/// two samples there is no interval to integrate, so the area is `0`.
#[must_use]
pub fn integral(series: &Series) -> f64 {
    let samples = series.samples();
    samples
        .windows(2)
        .map(|w| {
            let dt = (w[1].timestamp() - w[0].timestamp()) as f64;
            (w[0].value() + w[1].value()) / 2.0 * dt
        })
        .sum()
}

/// The time-weighted mean over the series: the trapezoidal [`integral`] divided
/// by the total time span. This is the "true" average a thermostat history
/// should report — long-held values count more than brief spikes.
///
/// Returns `None` when the span is zero (fewer than two distinct timestamps),
/// because a time-weighted mean is undefined without elapsed time.
#[must_use]
pub fn time_weighted_mean(series: &Series) -> Option<f64> {
    let span = series.span();
    if span == 0 {
        return None;
    }
    Some(integral(series) / span as f64)
}

/// End-to-end rate of change: `(last_value - first_value) / (last_t - first_t)`
/// in value-per-time-unit.
///
/// Returns `None` when there is no elapsed time (fewer than two distinct
/// timestamps) — a rate over zero time is undefined.
#[must_use]
pub fn rate_of_change(series: &Series) -> Option<f64> {
    let samples = series.samples();
    let first = samples.first()?;
    let last = samples.last()?;
    let dt = last.timestamp() - first.timestamp();
    if dt == 0 {
        return None;
    }
    Some((last.value() - first.value()) / dt as f64)
}

/// A run of "missing" time: the sensor produced no sample between two
/// consecutive samples that are further apart than the expected interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gap {
    /// Timestamp of the last sample before the silence.
    pub from: i64,
    /// Timestamp of the first sample after the silence.
    pub to: i64,
}

impl Gap {
    /// How long the silence lasted, in timestamp units.
    #[must_use]
    pub const fn duration(self) -> i64 {
        self.to - self.from
    }
}

/// Find every gap longer than `expected_interval` between consecutive samples.
///
/// A gap is reported when two neighbours are more than `expected_interval`
/// apart — i.e. at least one expected sample is missing. A non-positive
/// `expected_interval` means "no expectation", so no gaps are reported.
#[must_use]
pub fn find_gaps(series: &Series, expected_interval: i64) -> Vec<Gap> {
    if expected_interval <= 0 {
        return Vec::new();
    }
    series
        .samples()
        .windows(2)
        .filter_map(|w| {
            let from = w[0].timestamp();
            let to = w[1].timestamp();
            if to - from > expected_interval {
                Some(Gap { from, to })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: i64, v: f64) -> Sample {
        Sample::new(t, v).expect("finite test sample")
    }

    fn series(pairs: &[(i64, f64)]) -> Series {
        Series::sorted(pairs.iter().map(|&(t, v)| s(t, v)).collect())
    }

    #[test]
    fn summary_basic_stats() {
        let sum = summarize(&series(&[(0, 2.0), (1, 4.0), (2, 6.0)])).expect("non-empty");
        assert_eq!(sum.count, 3);
        assert_eq!(sum.min, 2.0);
        assert_eq!(sum.max, 6.0);
        assert_eq!(sum.mean, 4.0);
        assert_eq!(sum.sum, 12.0);
    }

    #[test]
    fn stddev_known_population() {
        // Values 2,4,4,4,5,5,7,9: mean 5, population stddev 2.
        let sum = summarize(&series(&[
            (0, 2.0),
            (1, 4.0),
            (2, 4.0),
            (3, 4.0),
            (4, 5.0),
            (5, 5.0),
            (6, 7.0),
            (7, 9.0),
        ]))
        .expect("non-empty");
        assert_eq!(sum.mean, 5.0);
        assert!((sum.stddev - 2.0).abs() < 1e-12, "stddev = {}", sum.stddev);
    }

    #[test]
    fn stddev_of_constant_is_zero() {
        let sum = summarize(&series(&[(0, 7.0), (1, 7.0), (2, 7.0)])).expect("non-empty");
        assert_eq!(sum.stddev, 0.0);
    }

    #[test]
    fn summary_empty_is_none() {
        assert_eq!(summarize(&Series::empty()), None);
    }

    #[test]
    fn integral_of_constant_is_rectangle() {
        // Constant 10 over t=0..100 -> area 10*100 = 1000.
        let area = integral(&series(&[(0, 10.0), (100, 10.0)]));
        assert!((area - 1000.0).abs() < 1e-9, "area = {area}");
    }

    #[test]
    fn integral_of_triangle_is_half_base_times_height() {
        // Ramp 0 -> 100 over t=0..10: triangle area = 0.5 * 10 * 100 = 500.
        let area = integral(&series(&[(0, 0.0), (10, 100.0)]));
        assert!((area - 500.0).abs() < 1e-9, "area = {area}");
    }

    #[test]
    fn integral_trapezoid_multiple_segments() {
        // y = x at t=0,1,2,3,4. Area under y=x from 0..4 = 8.
        let area = integral(&series(&[
            (0, 0.0),
            (1, 1.0),
            (2, 2.0),
            (3, 3.0),
            (4, 4.0),
        ]));
        assert!((area - 8.0).abs() < 1e-9, "area = {area}");
    }

    #[test]
    fn integral_single_or_empty_is_zero() {
        assert_eq!(integral(&Series::empty()), 0.0);
        assert_eq!(integral(&series(&[(5, 99.0)])), 0.0);
    }

    #[test]
    fn time_weighted_mean_weights_by_duration() {
        // 0 held 0..1 (1s), 10 held 1..10 (9s). Trapezoidal area:
        // seg1 (0+10)/2*1 = 5; seg2 (10+10)/2*9 = 90 -> 95 over span 10 = 9.5.
        let twm = time_weighted_mean(&series(&[(0, 0.0), (1, 10.0), (10, 10.0)]))
            .expect("non-zero span");
        assert!((twm - 9.5).abs() < 1e-9, "twm = {twm}");
    }

    #[test]
    fn time_weighted_mean_zero_span_is_none() {
        assert_eq!(time_weighted_mean(&series(&[(5, 1.0)])), None);
    }

    #[test]
    fn rate_of_change_rising() {
        // 18 -> 21 over 60s = +0.05 per unit.
        let rate = rate_of_change(&series(&[(0, 18.0), (60, 21.0)])).expect("non-zero dt");
        assert!((rate - 0.05).abs() < 1e-12, "rate = {rate}");
    }

    #[test]
    fn rate_of_change_falling_is_negative() {
        let rate = rate_of_change(&series(&[(0, 20.0), (10, 15.0)])).expect("non-zero dt");
        assert!(rate < 0.0);
    }

    #[test]
    fn rate_of_change_needs_elapsed_time() {
        assert_eq!(rate_of_change(&Series::empty()), None);
        assert_eq!(rate_of_change(&series(&[(7, 1.0)])), None);
    }

    #[test]
    fn find_gaps_detects_silence() {
        // Expected every 60s. 0,60,120 then a jump to 600 (a 480s gap).
        let gaps = find_gaps(
            &series(&[(0, 1.0), (60, 1.0), (120, 1.0), (600, 1.0)]),
            60,
        );
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0], Gap { from: 120, to: 600 });
        assert_eq!(gaps[0].duration(), 480);
    }

    #[test]
    fn find_gaps_none_when_regular() {
        let gaps = find_gaps(&series(&[(0, 1.0), (60, 1.0), (120, 1.0)]), 60);
        assert!(gaps.is_empty());
    }

    #[test]
    fn find_gaps_nonpositive_interval_reports_nothing() {
        let gaps = find_gaps(&series(&[(0, 1.0), (10_000, 1.0)]), 0);
        assert!(gaps.is_empty());
    }
}
