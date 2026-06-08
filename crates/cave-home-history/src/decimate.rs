//! Decimation for charts: shrink a long series to roughly `target` points
//! while keeping the shape a human would draw.
//!
//! The default is **LTTB** (Largest-Triangle-Three-Buckets): it always keeps
//! the first and last samples and, for each interior bucket, picks the one
//! sample that forms the largest-area triangle with the previously chosen point
//! and the average of the next bucket. That preserves peaks and troughs far
//! better than naive every-nth sampling — exactly what a "last week's
//! temperature" chart needs.
//!
//! [`min_max`] is the simpler alternative: keep both the lowest and highest
//! sample of each bucket. It guarantees no spike is ever clipped, at the cost
//! of up to twice as many points.

use crate::sample::{Sample, Series};

/// Largest-Triangle-Three-Buckets downsampling to about `target` points.
///
/// If the series already has `target` points or fewer (or `target < 3`), it is
/// returned unchanged — there is nothing to remove and the endpoints-plus-
/// interior contract still holds. The output always starts at the first sample
/// and ends at the last, preserves time order, and has exactly `target` points
/// when the input is larger than `target`.
#[must_use]
pub fn lttb(series: &Series, target: usize) -> Series {
    let data = series.samples();
    let n = data.len();
    if target < 3 || n <= target {
        return series.clone();
    }

    let mut out: Vec<Sample> = Vec::with_capacity(target);
    // Always keep the first point.
    out.push(data[0]);

    // Interior buckets divide the points between the fixed endpoints.
    let bucket_size = (n - 2) as f64 / (target - 2) as f64;
    let mut a = 0usize; // index of the last chosen point

    for i in 0..(target - 2) {
        // Range of the *next* bucket, used to compute its average point.
        let next_lo = ((i + 1) as f64 * bucket_size).floor() as usize + 1;
        let next_hi = (((i + 2) as f64 * bucket_size).floor() as usize + 1).min(n);
        let (avg_t, avg_v) = average_point(&data[next_lo.min(n)..next_hi.max(next_lo.min(n))]);

        // Range of the current bucket we are choosing a representative from.
        let cur_lo = (i as f64 * bucket_size).floor() as usize + 1;
        let cur_hi = (((i + 1) as f64 * bucket_size).floor() as usize + 1).min(n);

        let point_a = data[a];
        let mut best_area = -1.0;
        let mut best_idx = cur_lo;
        for (offset, candidate) in data[cur_lo..cur_hi].iter().enumerate() {
            let area = triangle_area(point_a, *candidate, avg_t, avg_v);
            if area > best_area {
                best_area = area;
                best_idx = cur_lo + offset;
            }
        }
        out.push(data[best_idx]);
        a = best_idx;
    }

    // Always keep the last point.
    out.push(data[n - 1]);
    Series::from_sorted(out)
}

/// Mean (timestamp, value) of a slice; `(0,0)` for an empty slice (only reached
/// at the tail where the next bucket collapses onto the final point).
fn average_point(slice: &[Sample]) -> (f64, f64) {
    if slice.is_empty() {
        return (0.0, 0.0);
    }
    let n = slice.len() as f64;
    let t: f64 = slice.iter().map(|s| s.timestamp() as f64).sum::<f64>() / n;
    let v: f64 = slice.iter().map(Sample::value).sum::<f64>() / n;
    (t, v)
}

/// Twice the area of the triangle formed by point `a`, a candidate, and the
/// next bucket's average point. (The factor of two and the absolute value do
/// not change which triangle is largest.)
fn triangle_area(a: Sample, candidate: Sample, avg_t: f64, avg_v: f64) -> f64 {
    let ax = a.timestamp() as f64;
    let ay = a.value();
    let bx = candidate.timestamp() as f64;
    let by = candidate.value();
    ((ax - avg_t) * (by - ay) - (ax - bx) * (avg_v - ay)).abs()
}

/// Simpler decimation: split the time range into `buckets` equal windows and
/// keep the lowest and highest sample of each. Never clips a spike.
///
/// Returns the input unchanged when `buckets` is zero or the series already has
/// two points or fewer. Output is time-ordered; within a bucket the earlier of
/// the min/max comes first.
#[must_use]
pub fn min_max(series: &Series, buckets: usize) -> Series {
    let data = series.samples();
    let n = data.len();
    if buckets == 0 || n <= 2 {
        return series.clone();
    }
    let bucket_size = (n as f64 / buckets as f64).ceil() as usize;
    let bucket_size = bucket_size.max(1);

    let mut out: Vec<Sample> = Vec::new();
    let mut start = 0usize;
    while start < n {
        let end = (start + bucket_size).min(n);
        let chunk = &data[start..end];
        if let (Some(lo), Some(hi)) = (
            chunk
                .iter()
                .min_by(|a, b| a.value().partial_cmp(&b.value()).unwrap_or(core::cmp::Ordering::Equal)),
            chunk
                .iter()
                .max_by(|a, b| a.value().partial_cmp(&b.value()).unwrap_or(core::cmp::Ordering::Equal)),
        ) {
            // Emit in timestamp order so the result is monotonic.
            if lo.timestamp() <= hi.timestamp() {
                out.push(*lo);
                if hi != lo {
                    out.push(*hi);
                }
            } else {
                out.push(*hi);
                out.push(*lo);
            }
        }
        start = end;
    }
    Series::from_sorted(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: i64, v: f64) -> Sample {
        Sample::new(t, v).expect("finite test sample")
    }

    fn ramp(n: usize) -> Series {
        Series::from_sorted((0..n).map(|i| s(i as i64, i as f64)).collect())
    }

    fn is_monotonic(series: &Series) -> bool {
        series
            .samples()
            .windows(2)
            .all(|w| w[0].timestamp() <= w[1].timestamp())
    }

    #[test]
    fn lttb_hits_target_count() {
        let down = lttb(&ramp(100), 10);
        assert_eq!(down.len(), 10);
    }

    #[test]
    fn lttb_preserves_endpoints() {
        let series = ramp(100);
        let down = lttb(&series, 12);
        assert_eq!(down.samples().first(), series.samples().first());
        assert_eq!(down.samples().last(), series.samples().last());
    }

    #[test]
    fn lttb_output_is_time_ordered() {
        let down = lttb(&ramp(500), 50);
        assert!(is_monotonic(&down), "LTTB output must keep time order");
    }

    #[test]
    fn lttb_returns_input_when_already_small() {
        let series = ramp(5);
        assert_eq!(lttb(&series, 10), series);
        assert_eq!(lttb(&series, 5), series);
    }

    #[test]
    fn lttb_target_below_three_returns_input() {
        let series = ramp(50);
        assert_eq!(lttb(&series, 2), series);
        assert_eq!(lttb(&series, 0), series);
    }

    #[test]
    fn lttb_keeps_a_spike() {
        // Flat line with one tall spike in the middle; LTTB to a few points
        // must retain the spike value, which naive sampling could miss.
        let mut pts: Vec<Sample> = (0..100).map(|i| s(i as i64, 0.0)).collect();
        pts[50] = s(50, 1000.0);
        let series = Series::from_sorted(pts);
        let down = lttb(&series, 8);
        let kept_spike = down
            .samples()
            .iter()
            .any(|p| (p.value() - 1000.0).abs() < 1e-9);
        assert!(kept_spike, "LTTB dropped the only meaningful feature");
    }

    #[test]
    fn min_max_keeps_extremes_and_is_monotonic() {
        let mut pts: Vec<Sample> = (0..40).map(|i| s(i as i64, (i % 5) as f64)).collect();
        pts[20] = s(20, -100.0);
        pts[21] = s(21, 100.0);
        let series = Series::from_sorted(pts);
        let down = min_max(&series, 4);
        assert!(is_monotonic(&down));
        let lo = down
            .samples()
            .iter()
            .map(Sample::value)
            .fold(f64::INFINITY, f64::min);
        let hi = down
            .samples()
            .iter()
            .map(Sample::value)
            .fold(f64::NEG_INFINITY, f64::max);
        assert_eq!(lo, -100.0, "min spike must survive");
        assert_eq!(hi, 100.0, "max spike must survive");
    }

    #[test]
    fn min_max_returns_input_for_tiny_series() {
        let series = ramp(2);
        assert_eq!(min_max(&series, 4), series);
        assert_eq!(min_max(&ramp(50), 0), ramp(50));
    }
}
