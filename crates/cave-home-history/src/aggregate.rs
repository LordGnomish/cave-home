//! Aggregation and downsampling: fold many raw samples into one value per
//! fixed time window.
//!
//! Given a window width `w` and an [`Aggregator`], the engine partitions the
//! time line into half-open buckets `[k·w, (k+1)·w)` anchored at epoch 0 and
//! collapses every bucket that contains samples to a single point. The result
//! is itself a [`Series`], stamped at the **start** of each bucket — the shape
//! a chart or a rollup store ([`crate::retention`]) consumes.
//!
//! Empty buckets (gaps) produce **no** point; the downsampled series simply
//! skips them. Detecting those gaps explicitly is [`crate::stats::find_gaps`].

use crate::sample::{Sample, Series};

/// How to collapse the samples in one bucket to a single value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aggregator {
    /// Arithmetic mean of the bucket's values.
    Mean,
    /// Smallest value in the bucket.
    Min,
    /// Largest value in the bucket.
    Max,
    /// Sum of the bucket's values.
    Sum,
    /// How many samples fell in the bucket (the value is the count).
    Count,
    /// Earliest sample's value (by timestamp; ties keep input order).
    First,
    /// Latest sample's value.
    Last,
    /// Median — the middle value, or the mean of the two middle values.
    Median,
    /// 95th percentile (linear interpolation between ranks).
    P95,
}

impl Aggregator {
    /// Collapse a non-empty, time-ordered slice of samples to one value.
    ///
    /// Returns `None` only for an empty slice — every aggregator is total over
    /// a bucket that contains at least one sample.
    #[must_use]
    pub fn apply(self, bucket: &[Sample]) -> Option<f64> {
        if bucket.is_empty() {
            return None;
        }
        let value = match self {
            Self::Mean => {
                let sum: f64 = bucket.iter().map(Sample::value).sum();
                sum / bucket.len() as f64
            }
            Self::Min => bucket
                .iter()
                .map(Sample::value)
                .fold(f64::INFINITY, f64::min),
            Self::Max => bucket
                .iter()
                .map(Sample::value)
                .fold(f64::NEG_INFINITY, f64::max),
            Self::Sum => bucket.iter().map(Sample::value).sum(),
            Self::Count => bucket.len() as f64,
            Self::First => bucket.first().map_or(0.0, Sample::value),
            Self::Last => bucket.last().map_or(0.0, Sample::value),
            Self::Median => percentile(bucket, 50.0),
            Self::P95 => percentile(bucket, 95.0),
        };
        Some(value)
    }
}

/// Linear-interpolation percentile over a bucket's values.
///
/// `p` is in `0..=100`. Values are copied and sorted (the bucket order is by
/// timestamp, not value). With one sample the percentile is that sample.
fn percentile(bucket: &[Sample], p: f64) -> f64 {
    let mut vals: Vec<f64> = bucket.iter().map(Sample::value).collect();
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
    let n = vals.len();
    if n == 1 {
        return vals[0];
    }
    // Rank within [0, n-1] using the common "linear interpolation between
    // closest ranks" definition.
    let rank = (p / 100.0) * (n - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    if lo == hi {
        return vals[lo];
    }
    let frac = rank - lo as f64;
    vals[lo] + (vals[hi] - vals[lo]) * frac
}

/// Downsample a series into fixed-width buckets, collapsing each non-empty
/// bucket with `agg`. Empty buckets are skipped (no fabricated points).
///
/// The output is time-ordered and stamped at each bucket's **start**. A
/// `window` of zero or less is meaningless, so the input is returned unchanged
/// (every sample is its own bucket would diverge); callers should pass a
/// positive width.
///
/// # Panics
/// Never — values are guaranteed finite by [`Sample::new`].
#[must_use]
pub fn downsample(series: &Series, window: i64, agg: Aggregator) -> Series {
    if window <= 0 || series.is_empty() {
        return series.clone();
    }
    let mut out: Vec<Sample> = Vec::new();
    let mut bucket: Vec<Sample> = Vec::new();
    let mut current_start: Option<i64> = None;

    for &sample in series.samples() {
        let start = bucket_start(sample.timestamp(), window);
        match current_start {
            Some(cs) if cs == start => bucket.push(sample),
            _ => {
                flush(&mut out, &bucket, current_start, agg);
                bucket.clear();
                bucket.push(sample);
                current_start = Some(start);
            }
        }
    }
    flush(&mut out, &bucket, current_start, agg);
    Series::from_sorted(out)
}

fn flush(out: &mut Vec<Sample>, bucket: &[Sample], start: Option<i64>, agg: Aggregator) {
    if let (Some(start), Some(value)) = (start, agg.apply(bucket)) {
        if let Ok(point) = Sample::new(start, value) {
            out.push(point);
        }
    }
}

/// The start timestamp of the half-open bucket `[k·w, (k+1)·w)` containing `t`,
/// anchored at epoch 0 and correct for negative timestamps (floor division).
#[must_use]
pub fn bucket_start(t: i64, window: i64) -> i64 {
    if window <= 0 {
        return t;
    }
    let rem = t.rem_euclid(window);
    t - rem
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: i64, v: f64) -> Sample {
        Sample::new(t, v).expect("finite test sample")
    }

    fn bucket(vals: &[f64]) -> Vec<Sample> {
        vals.iter()
            .enumerate()
            .map(|(i, &v)| s(i as i64, v))
            .collect()
    }

    #[test]
    fn bucket_start_floors_to_window() {
        assert_eq!(bucket_start(0, 60), 0);
        assert_eq!(bucket_start(59, 60), 0);
        assert_eq!(bucket_start(60, 60), 60);
        assert_eq!(bucket_start(125, 60), 120);
    }

    #[test]
    fn bucket_start_handles_negative_timestamps() {
        // Floor division: -1 belongs to bucket [-60, 0).
        assert_eq!(bucket_start(-1, 60), -60);
        assert_eq!(bucket_start(-60, 60), -60);
        assert_eq!(bucket_start(-61, 60), -120);
    }

    #[test]
    fn mean_aggregator() {
        assert_eq!(Aggregator::Mean.apply(&bucket(&[2.0, 4.0, 6.0])), Some(4.0));
    }

    #[test]
    fn min_max_aggregators() {
        let b = bucket(&[3.0, -1.0, 7.0, 2.0]);
        assert_eq!(Aggregator::Min.apply(&b), Some(-1.0));
        assert_eq!(Aggregator::Max.apply(&b), Some(7.0));
    }

    #[test]
    fn sum_and_count_aggregators() {
        let b = bucket(&[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(Aggregator::Sum.apply(&b), Some(10.0));
        assert_eq!(Aggregator::Count.apply(&b), Some(4.0));
    }

    #[test]
    fn first_and_last_aggregators() {
        // bucket() stamps ascending timestamps, so first/last are by time.
        let b = bucket(&[10.0, 20.0, 30.0]);
        assert_eq!(Aggregator::First.apply(&b), Some(10.0));
        assert_eq!(Aggregator::Last.apply(&b), Some(30.0));
    }

    #[test]
    fn median_odd_and_even() {
        assert_eq!(Aggregator::Median.apply(&bucket(&[5.0, 1.0, 3.0])), Some(3.0));
        // Even count: mean of the two middle (sorted: 1,3,5,7 -> (3+5)/2).
        assert_eq!(
            Aggregator::Median.apply(&bucket(&[7.0, 1.0, 5.0, 3.0])),
            Some(4.0)
        );
    }

    #[test]
    fn p95_interpolates_between_ranks() {
        // 0..=100 step 1, n=101: rank = 0.95*100 = 95 exactly -> value 95.
        let vals: Vec<f64> = (0..=100).map(|i| i as f64).collect();
        assert_eq!(Aggregator::P95.apply(&bucket(&vals)), Some(95.0));
    }

    #[test]
    fn single_sample_percentiles_are_that_sample() {
        let b = bucket(&[42.0]);
        assert_eq!(Aggregator::Median.apply(&b), Some(42.0));
        assert_eq!(Aggregator::P95.apply(&b), Some(42.0));
    }

    #[test]
    fn empty_bucket_yields_none() {
        assert_eq!(Aggregator::Mean.apply(&[]), None);
        assert_eq!(Aggregator::Count.apply(&[]), None);
        assert_eq!(Aggregator::P95.apply(&[]), None);
    }

    #[test]
    fn downsample_buckets_by_window() {
        // Window 60. Bucket [0,60): 10,20 -> mean 15. Bucket [60,120): 40.
        let series = Series::sorted(vec![s(0, 10.0), s(30, 20.0), s(70, 40.0)]);
        let down = downsample(&series, 60, Aggregator::Mean);
        assert_eq!(down.len(), 2);
        assert_eq!(down.samples()[0], s(0, 15.0));
        assert_eq!(down.samples()[1], s(60, 40.0));
    }

    #[test]
    fn downsample_skips_empty_buckets_no_fabricated_points() {
        // Samples at t=0 and t=180 with window 60: buckets [60,120) and
        // [120,180) are empty and MUST NOT appear in the output.
        let series = Series::sorted(vec![s(0, 1.0), s(180, 9.0)]);
        let down = downsample(&series, 60, Aggregator::Mean);
        assert_eq!(down.len(), 2);
        let starts: Vec<i64> = down
            .samples()
            .iter()
            .map(Sample::timestamp)
            .collect();
        assert_eq!(starts, vec![0, 180]);
    }

    #[test]
    fn downsample_empty_series_is_empty() {
        let down = downsample(&Series::empty(), 60, Aggregator::Mean);
        assert!(down.is_empty());
    }

    #[test]
    fn downsample_nonpositive_window_returns_input() {
        let series = Series::sorted(vec![s(0, 1.0), s(5, 2.0)]);
        assert_eq!(downsample(&series, 0, Aggregator::Mean), series);
        assert_eq!(downsample(&series, -10, Aggregator::Mean), series);
    }

    #[test]
    fn downsample_last_picks_latest_in_each_bucket() {
        let series = Series::sorted(vec![s(0, 1.0), s(10, 2.0), s(70, 3.0)]);
        let down = downsample(&series, 60, Aggregator::Last);
        assert_eq!(down.samples()[0].value(), 2.0);
        assert_eq!(down.samples()[1].value(), 3.0);
    }
}
