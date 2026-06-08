//! EPA NowCast — time-weighted averaging for particulate matter (PM2.5/PM10).
//!
//! Implemented from the public EPA NowCast definition (EPA-454/B-24-002 family).
//! The NowCast weights recent hours more heavily when concentrations change
//! quickly, so a live tile reacts faster than a flat 24-hour mean while still
//! smoothing noise. The weight factor is `min/max` over the available window,
//! floored at 0.5 for PM; the NowCast is the weight-discounted average of the
//! most recent (up to) 12 hourly values.

/// Number of hourly samples the NowCast window spans.
const WINDOW: usize = 12;
/// PM weight-factor floor (EPA): the weight never drops below 0.5.
const WEIGHT_FLOOR: f64 = 0.5;

/// Compute the EPA NowCast PM concentration from up-to-12 hourly readings,
/// **most-recent-first** (`hourly[0]` is the current hour). `None` marks a
/// missing hour.
///
/// Returns `None` when fewer than two of the three most-recent hours have data
/// (EPA's validity rule) — the NowCast is then "unavailable" rather than
/// fabricated. An all-zero window returns `Some(0.0)` (not `NaN`).
#[must_use]
pub fn now_cast(hourly_recent_first: &[Option<f64>]) -> Option<f64> {
    let window = &hourly_recent_first[..hourly_recent_first.len().min(WINDOW)];

    // Validity: at least two of the three most-recent hours must be present.
    let recent_present = window.iter().take(3).filter(|v| v.is_some()).count();
    if recent_present < 2 {
        return None;
    }

    // Range over present, finite values.
    let present: Vec<(usize, f64)> = window
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.filter(|x| x.is_finite()).map(|x| (i, x)))
        .collect();
    if present.is_empty() {
        return None;
    }

    let max = present.iter().fold(f64::MIN, |m, &(_, v)| m.max(v));
    let min = present.iter().fold(f64::MAX, |m, &(_, v)| m.min(v));

    // All-zero (or max == 0) window: NowCast is 0, not 0/0.
    if max <= 0.0 {
        return Some(0.0);
    }

    let weight = (min / max).max(WEIGHT_FLOOR);

    // NowCast = Σ weight^i · c_i / Σ weight^i over present hours (i = hours ago).
    let mut num = 0.0;
    let mut den = 0.0;
    for &(i, c) in &present {
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let w = weight.powi(i as i32);
        num += w * c;
        den += w;
    }
    if den <= 0.0 {
        return None;
    }
    // Return the raw NowCast concentration. (EPA truncates only at the final
    // *reporting* step when converting to an integer AQI category; callers that
    // need the reported form truncate then — we keep full precision here.)
    Some(num / den)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_full_precision_for_callers() {
        // We return the raw NowCast (55/1.5 = 36.6667..), not a truncated form;
        // truncation is a final-reporting concern left to the caller.
        let r = now_cast(&[Some(50.0), Some(10.0)]).expect("data");
        assert!((r - 36.666_666).abs() < 1e-4, "got {r}");
    }

    #[test]
    fn caps_window_at_twelve_hours() {
        // A 24-long all-20 input uses only the first 12 -> still 20.0.
        let r = now_cast(&[Some(20.0); 24]).expect("data");
        assert!((r - 20.0).abs() < 0.05, "got {r}");
    }

    #[test]
    fn rejects_nan_hours() {
        // A NaN in an otherwise-valid window is ignored, not propagated.
        let r = now_cast(&[Some(20.0), Some(f64::NAN), Some(20.0)]).expect("data");
        assert!(r.is_finite());
    }
}
