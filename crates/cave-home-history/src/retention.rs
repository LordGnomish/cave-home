//! Retention and rollup classification: decide, as a function of "now", which
//! samples stay at full resolution, which get rolled up to a coarser interval,
//! and which fall off the end entirely.
//!
//! A [`RetentionPolicy`] is an ordered ladder of [`Tier`]s, e.g. *raw for 7
//! days, then 1-minute averages for 30 days, then 1-hour averages for a year*.
//! Given a sample's age (now − timestamp) the policy says which tier it belongs
//! to via [`RetentionPolicy::classify`]; [`RetentionPolicy::partition`] sorts a
//! whole series into keep / roll-up / evict.
//!
//! This module decides *what* to do. Actually downsampling the roll-up tiers is
//! [`crate::aggregate::downsample`]; persisting and deleting is the storage
//! engine (Phase 1b, ADR-023). No clock here — "now" is supplied by the caller.

use crate::aggregate::Aggregator;
use crate::sample::{Sample, Series};

/// One rung of a retention ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tier {
    /// How far back, in timestamp units, this tier extends from "now". A sample
    /// belongs to the first tier whose `max_age` it is within.
    pub max_age: i64,
    /// The resolution this tier is stored at, in timestamp units. `0` means
    /// "raw" — keep every sample as-is, no roll-up.
    pub rollup_interval: i64,
}

impl Tier {
    /// A raw (full-resolution) tier covering the most recent `max_age`.
    #[must_use]
    pub const fn raw(max_age: i64) -> Self {
        Self { max_age, rollup_interval: 0 }
    }

    /// A rolled-up tier: keep `rollup_interval`-wide aggregates back to
    /// `max_age`.
    #[must_use]
    pub const fn rollup(max_age: i64, rollup_interval: i64) -> Self {
        Self { max_age, rollup_interval }
    }

    /// Whether this tier keeps samples at full resolution.
    #[must_use]
    pub const fn is_raw(self) -> bool {
        self.rollup_interval <= 0
    }
}

/// What should happen to a single sample under a policy at a given "now".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// Keep at full resolution (it is within a raw tier).
    KeepRaw,
    /// Roll up to the given interval (it is within a rolled-up tier).
    RollUp(i64),
    /// Older than every tier — evict.
    Evict,
}

/// An ordered retention ladder.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RetentionPolicy {
    tiers: Vec<Tier>,
}

impl RetentionPolicy {
    /// Build a policy from tiers. They are sorted by `max_age` ascending so
    /// classification picks the *finest* tier a sample qualifies for, whatever
    /// order the caller supplied.
    #[must_use]
    pub fn new(mut tiers: Vec<Tier>) -> Self {
        tiers.sort_by_key(|t| t.max_age);
        Self { tiers }
    }

    /// The tiers, finest (smallest `max_age`) first.
    #[must_use]
    pub fn tiers(&self) -> &[Tier] {
        &self.tiers
    }

    /// The farthest-back any tier reaches; samples older than this are evicted.
    /// `0` for an empty policy (which evicts everything with positive age).
    #[must_use]
    pub fn horizon(&self) -> i64 {
        self.tiers.last().map_or(0, |t| t.max_age)
    }

    /// Classify a single sample given the current `now`.
    ///
    /// Age is `now - timestamp`. A future or present sample (age ≤ 0) is always
    /// kept raw — the newest possible data. Otherwise the first tier whose
    /// `max_age` covers the age wins; if none does, the sample is evicted.
    #[must_use]
    pub fn classify(&self, sample_timestamp: i64, now: i64) -> Disposition {
        let age = now - sample_timestamp;
        if age <= 0 {
            return Disposition::KeepRaw;
        }
        for tier in &self.tiers {
            if age <= tier.max_age {
                return if tier.is_raw() {
                    Disposition::KeepRaw
                } else {
                    Disposition::RollUp(tier.rollup_interval)
                };
            }
        }
        Disposition::Evict
    }

    /// Partition a whole series at the given `now` into the three buckets.
    #[must_use]
    pub fn partition(&self, series: &Series, now: i64) -> Partitioned {
        let mut keep_raw = Vec::new();
        let mut roll_up = Vec::new();
        let mut evict = Vec::new();
        for &sample in series.samples() {
            match self.classify(sample.timestamp(), now) {
                Disposition::KeepRaw => keep_raw.push(sample),
                Disposition::RollUp(_) => roll_up.push(sample),
                Disposition::Evict => evict.push(sample),
            }
        }
        Partitioned {
            keep_raw: Series::from_sorted(keep_raw),
            roll_up: Series::from_sorted(roll_up),
            evict: Series::from_sorted(evict),
        }
    }
}

/// The result of applying a policy to a series.
#[derive(Debug, Clone, PartialEq)]
pub struct Partitioned {
    /// Samples to keep at full resolution.
    pub keep_raw: Series,
    /// Samples destined for a rolled-up tier (feed to
    /// [`crate::aggregate::downsample`]).
    pub roll_up: Series,
    /// Samples past the policy horizon — to be deleted.
    pub evict: Series,
}

impl Partitioned {
    /// Roll up the [`Partitioned::roll_up`] samples to `interval`-wide buckets
    /// using `agg`. A convenience over calling
    /// [`crate::aggregate::downsample`] directly with a tier's interval.
    #[must_use]
    pub fn rolled(&self, interval: i64, agg: Aggregator) -> Series {
        crate::aggregate::downsample(&self.roll_up, interval, agg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: i64, v: f64) -> Sample {
        Sample::new(t, v).expect("finite test sample")
    }

    // A realistic ladder, in seconds: raw 7d, 1-min for 30d, 1-hr for 1y.
    const DAY: i64 = 86_400;
    fn ladder() -> RetentionPolicy {
        RetentionPolicy::new(vec![
            Tier::raw(7 * DAY),
            Tier::rollup(30 * DAY, 60),
            Tier::rollup(365 * DAY, 3600),
        ])
    }

    #[test]
    fn new_sorts_tiers_finest_first() {
        let p = RetentionPolicy::new(vec![
            Tier::rollup(365 * DAY, 3600),
            Tier::raw(7 * DAY),
            Tier::rollup(30 * DAY, 60),
        ]);
        let ages: Vec<i64> = p.tiers().iter().map(|t| t.max_age).collect();
        assert_eq!(ages, vec![7 * DAY, 30 * DAY, 365 * DAY]);
    }

    #[test]
    fn recent_sample_kept_raw() {
        let now = 1_000_000_000;
        // 1 day old -> within the raw 7-day tier.
        assert_eq!(p_classify(now - DAY, now), Disposition::KeepRaw);
    }

    #[test]
    fn present_or_future_sample_kept_raw() {
        let now = 1_000_000_000;
        assert_eq!(p_classify(now, now), Disposition::KeepRaw);
        assert_eq!(p_classify(now + 5, now), Disposition::KeepRaw);
    }

    #[test]
    fn mid_age_sample_rolls_up_to_one_minute() {
        let now = 1_000_000_000;
        // 14 days old -> past raw(7d), within rollup(30d, 60).
        assert_eq!(p_classify(now - 14 * DAY, now), Disposition::RollUp(60));
    }

    #[test]
    fn old_sample_rolls_up_to_one_hour() {
        let now = 1_000_000_000;
        // 100 days old -> within rollup(365d, 3600).
        assert_eq!(p_classify(now - 100 * DAY, now), Disposition::RollUp(3600));
    }

    #[test]
    fn past_horizon_sample_is_evicted() {
        let now = 1_000_000_000;
        // 2 years old -> past the 1-year horizon.
        assert_eq!(p_classify(now - 730 * DAY, now), Disposition::Evict);
    }

    #[test]
    fn tier_boundary_is_inclusive_on_finer_tier() {
        let now = 1_000_000_000;
        // Exactly 7 days old -> still raw (age <= max_age).
        assert_eq!(p_classify(now - 7 * DAY, now), Disposition::KeepRaw);
        // One second past -> next tier.
        assert_eq!(p_classify(now - (7 * DAY + 1), now), Disposition::RollUp(60));
    }

    #[test]
    fn horizon_is_oldest_tier() {
        assert_eq!(ladder().horizon(), 365 * DAY);
        assert_eq!(RetentionPolicy::default().horizon(), 0);
    }

    #[test]
    fn empty_policy_evicts_aged_keeps_present() {
        let p = RetentionPolicy::default();
        assert_eq!(p.classify(100, 100), Disposition::KeepRaw);
        assert_eq!(p.classify(100, 200), Disposition::Evict);
    }

    #[test]
    fn partition_splits_a_series_three_ways() {
        let now = 1_000_000_000;
        let series = Series::sorted(vec![
            s(now - DAY, 1.0),           // raw
            s(now - 14 * DAY, 2.0),      // 1-min rollup
            s(now - 100 * DAY, 3.0),     // 1-hr rollup
            s(now - 730 * DAY, 4.0),     // evict
        ]);
        let parts = ladder().partition(&series, now);
        assert_eq!(parts.keep_raw.len(), 1);
        assert_eq!(parts.roll_up.len(), 2);
        assert_eq!(parts.evict.len(), 1);
        assert_eq!(parts.evict.samples()[0].value(), 4.0);
    }

    #[test]
    fn partition_then_roll_up_downsamples() {
        let now = 1_000_000_000;
        // Two old samples 30s apart in the same minute-bucket, both inside the
        // 1-min rollup tier (~20 days old). now - 20*DAY is a multiple of 60,
        // so both land in bucket [now-20*DAY, now-20*DAY+60).
        let base = crate::aggregate::bucket_start(now - 20 * DAY, 60);
        let series = Series::sorted(vec![s(base, 10.0), s(base + 30, 20.0)]);
        let parts = ladder().partition(&series, now);
        assert_eq!(parts.roll_up.len(), 2);
        let rolled = parts.rolled(60, Aggregator::Mean);
        assert_eq!(rolled.len(), 1);
        assert_eq!(rolled.samples()[0].value(), 15.0);
    }

    fn p_classify(ts: i64, now: i64) -> Disposition {
        ladder().classify(ts, now)
    }
}
