//! Simple, pure trend classification over a caller-supplied series.
//!
//! Given a short slice of recent values (e.g. a week of daily step counts),
//! [`classify_trend`] reports whether the series is [`Trend::Improving`],
//! [`Trend::Steady`], or [`Trend::Declining`] by comparing the average of the
//! first half against the average of the second half. The comparison uses a
//! relative dead-band so small wobble reads as "steady" rather than noise.
//!
//! "Improving" means "the number is going up". For most wellness metrics (steps,
//! active minutes, sleep) up is the friendly direction, so callers can present
//! the trend directly. A caller tracking a metric where down is the friendly
//! direction can simply invert the labelling at the edge.

/// The direction of a short series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trend {
    /// The recent half is meaningfully higher than the earlier half.
    Improving,
    /// The two halves are within the dead-band of each other.
    Steady,
    /// The recent half is meaningfully lower than the earlier half.
    Declining,
}

/// Relative change (as a fraction of the earlier average) below which a series
/// is treated as steady rather than improving/declining. 5% wobble is noise.
const DEAD_BAND: f64 = 0.05;

/// Classify the direction of a series of values.
///
/// Splits the series into an earlier and a recent half (a lone middle sample in
/// an odd-length series is shared by both halves) and compares their averages.
/// An empty or single-element series has no direction and reads [`Trend::Steady`].
#[must_use]
pub fn classify_trend(series: &[f64]) -> Trend {
    if series.len() < 2 {
        return Trend::Steady;
    }
    // Split into halves; for odd lengths the middle element is shared so both
    // halves stay non-empty and balanced.
    let mid = series.len() / 2;
    let first = &series[..mid + (series.len() % 2)];
    let second = &series[mid..];

    let earlier = average(first);
    let recent = average(second);

    // Compare on an absolute scale when the baseline is ~zero, otherwise on a
    // relative scale so the dead-band is proportional to the metric.
    let diff = recent - earlier;
    let scale = earlier.abs().max(1.0);
    let relative = diff / scale;

    if relative > DEAD_BAND {
        Trend::Improving
    } else if relative < -DEAD_BAND {
        Trend::Declining
    } else {
        Trend::Steady
    }
}

/// Mean of a non-empty slice. Returns 0.0 for an empty slice.
#[must_use]
fn average(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let sum: f64 = values.iter().sum();
    // A wellness series is a handful of days; the count fits f64 exactly.
    let count = u32::try_from(values.len()).unwrap_or(u32::MAX);
    sum / f64::from(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rising_series_improves() {
        let week = [4_000.0, 5_000.0, 6_000.0, 7_000.0, 8_000.0, 9_000.0];
        assert_eq!(classify_trend(&week), Trend::Improving);
    }

    #[test]
    fn falling_series_declines() {
        let week = [9_000.0, 8_000.0, 7_000.0, 6_000.0, 5_000.0, 4_000.0];
        assert_eq!(classify_trend(&week), Trend::Declining);
    }

    #[test]
    fn flat_series_is_steady() {
        let week = [8_000.0, 8_100.0, 7_950.0, 8_050.0, 8_000.0, 8_020.0];
        assert_eq!(classify_trend(&week), Trend::Steady);
    }

    #[test]
    fn small_wobble_inside_dead_band_is_steady() {
        // ~2% lift, under the 5% dead-band.
        let series = [10_000.0, 10_000.0, 10_200.0, 10_200.0];
        assert_eq!(classify_trend(&series), Trend::Steady);
    }

    #[test]
    fn odd_length_series_shares_middle() {
        let series = [3_000.0, 6_000.0, 9_000.0];
        assert_eq!(classify_trend(&series), Trend::Improving);
    }

    #[test]
    fn degenerate_series_are_steady() {
        assert_eq!(classify_trend(&[]), Trend::Steady);
        assert_eq!(classify_trend(&[5_000.0]), Trend::Steady);
    }
}
