// SPDX-License-Identifier: Apache-2.0
//! A standard 5-field cron expression parser + next-fire computation.
//!
//! Behavioural reimplementation of the `robfig/cron` *standard* parser (the
//! exact dependency `pkg/controller/cronjob` uses to interpret a `CronJob`'s
//! `spec.schedule`). Fields, in order:
//!
//! ```text
//! ┌───────────── minute        (0-59)
//! │ ┌─────────── hour          (0-23)
//! │ │ ┌───────── day of month  (1-31)
//! │ │ │ ┌─────── month         (1-12)
//! │ │ │ │ ┌───── day of week   (0-6, Sunday = 0 or 7)
//! │ │ │ │ │
//! * * * * *
//! ```
//!
//! Each field supports `*`, a single value, `a-b` ranges, `*/n` and `a-b/n`
//! steps, and comma lists of those. Day-of-month and day-of-week follow the
//! standard-cron OR rule: when **both** are restricted (neither is `*`), a day
//! fires if **either** matches; when only one is restricted, only that one
//! constrains.
//!
//! Time is computed in **UTC** over caller-supplied epoch seconds — `std` only,
//! no clock read and no external date crate (a small civil-time conversion lives
//! in [`civil`]). [`CronSchedule::next_after`] returns the first matching
//! instant strictly **after** the given time, which is exactly the
//! `getNextScheduleTime` contract the `CronJob` controller needs.

use std::fmt;

mod civil;

/// A parse failure for a cron expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CronError {
    /// The expression did not have exactly five whitespace-separated fields;
    /// carries the count actually found.
    FieldCount(usize),
    /// A field was syntactically or semantically invalid (out of range,
    /// inverted range, zero step, non-numeric, …); carries the offending field.
    Field(String),
}

impl fmt::Display for CronError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FieldCount(n) => write!(f, "expected 5 cron fields, found {n}"),
            Self::Field(s) => write!(f, "invalid cron field: {s:?}"),
        }
    }
}

impl std::error::Error for CronError {}

/// A parsed cron schedule: one allowed-value bitset per field.
///
/// Bit `i` of a field's mask is set when value `i` is allowed. Day-of-week is
/// normalised to `0..=6` with Sunday = 0 (a literal `7` folds to `0`). The
/// `dom_restricted` / `dow_restricted` flags record whether each day field was
/// a wildcard, so [`Self::day_matches`] can apply the standard OR rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronSchedule {
    minute: u64,
    hour: u64,
    dom: u64,
    month: u64,
    dow: u64,
    dom_restricted: bool,
    dow_restricted: bool,
}

impl CronSchedule {
    /// Parse a 5-field cron expression.
    ///
    /// # Errors
    /// [`CronError::FieldCount`] if not exactly five fields; [`CronError::Field`]
    /// if any field is malformed or out of range.
    pub fn parse(expr: &str) -> Result<Self, CronError> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        if fields.len() != 5 {
            return Err(CronError::FieldCount(fields.len()));
        }
        let minute = parse_field(fields[0], 0, 59, false)?;
        let hour = parse_field(fields[1], 0, 23, false)?;
        let dom = parse_field(fields[2], 1, 31, false)?;
        let month = parse_field(fields[3], 1, 12, false)?;
        let dow = parse_field(fields[4], 0, 6, true)?;
        Ok(Self {
            minute,
            hour,
            dom,
            month,
            dow,
            dom_restricted: fields[2] != "*",
            dow_restricted: fields[4] != "*",
        })
    }

    /// The first instant (epoch seconds, UTC) that matches this schedule and is
    /// strictly **after** `after`.
    ///
    /// Searches minute by minute from the start of the next minute, advancing by
    /// whole days when the day does not match (so a sparse schedule like
    /// "Feb 29" terminates in a handful of day-steps per year, not 1440 per day).
    /// Bounded to a ~5-year horizon, beyond which it returns `after` unchanged —
    /// matching robfig's "give up rather than spin" behaviour for unsatisfiable
    /// schedules.
    #[must_use]
    pub fn next_after(&self, after: i64) -> i64 {
        // Start at the top of the minute strictly after `after`.
        let mut t = (after - after.rem_euclid(60)) + 60;
        // ~5 years of minutes is an ample upper bound; unsatisfiable schedules
        // (e.g. Feb 30) fall out here instead of looping forever.
        let limit = t + 5 * 366 * 86_400;
        while t < limit {
            let dt = civil::Civil::from_epoch(t);
            // Month must match first; if not, jump to the 1st of the next month.
            if !bit(self.month, u32::from(dt.month)) {
                t = civil::start_of_next_month(&dt);
                continue;
            }
            // Day must match (DOM/DOW OR rule); if not, jump a whole day.
            if !self.day_matches(&dt) {
                t = civil::start_of_next_day(&dt);
                continue;
            }
            // Day is right: check time-of-day fields.
            if bit(self.hour, u32::from(dt.hour)) && bit(self.minute, u32::from(dt.minute)) {
                return t;
            }
            t += 60;
        }
        after
    }

    /// Whether `dt`'s day satisfies the day-of-month / day-of-week fields under
    /// the standard-cron OR rule.
    fn day_matches(&self, dt: &civil::Civil) -> bool {
        let day_of_month_ok = bit(self.dom, u32::from(dt.day));
        let weekday_ok = bit(self.dow, u32::from(dt.weekday)); // 0=Sun..6=Sat
        match (self.dom_restricted, self.dow_restricted) {
            (true, true) => day_of_month_ok || weekday_ok, // either matches
            (true, false) => day_of_month_ok,
            (false, true) => weekday_ok,
            (false, false) => true, // both wildcards: every day
        }
    }
}

/// `true` if bit `n` is set in `mask`.
const fn bit(mask: u64, n: u32) -> bool {
    mask & (1u64 << n) != 0
}

/// Parse one cron field into an allowed-value bitset over `[lo, hi]`.
///
/// `dow` enables the Sunday-as-7 fold (value `7` → bit `0`). Supports `*`,
/// `a`, `a-b`, `*/n`, `a-b/n`, and comma lists thereof.
fn parse_field(field: &str, lo: u32, hi: u32, dow: bool) -> Result<u64, CronError> {
    let mut mask = 0u64;
    for part in field.split(',') {
        mask |= parse_part(part, lo, hi, dow).ok_or_else(|| CronError::Field(field.to_owned()))?;
    }
    if mask == 0 {
        return Err(CronError::Field(field.to_owned()));
    }
    Ok(mask)
}

/// Parse a single comma-separated atom (`*`, `a`, `a-b`, with optional `/n`).
fn parse_part(part: &str, lo: u32, hi: u32, dow: bool) -> Option<u64> {
    let (range, step) = match part.split_once('/') {
        Some((r, s)) => {
            let step: u32 = s.parse().ok()?;
            if step == 0 {
                return None;
            }
            (r, step)
        }
        None => (part, 1),
    };

    // Resolve the [start, end] the step ranges over.
    let (start, end) = if range == "*" {
        (lo, hi)
    } else if let Some((a, b)) = range.split_once('-') {
        let a = fold_dow(a.parse().ok()?, dow);
        let b = fold_dow(b.parse().ok()?, dow);
        if a > b || a < lo || b > hi {
            return None;
        }
        (a, b)
    } else {
        let v = fold_dow(range.parse().ok()?, dow);
        if v < lo || v > hi {
            return None;
        }
        // A bare single value ignores any step (`5/2` is unusual; treat the
        // single value as the start of an open range only when `*/n` form).
        (v, v)
    };

    let mut mask = 0u64;
    let mut v = start;
    while v <= end {
        mask |= 1u64 << v;
        v += step;
    }
    Some(mask)
}

/// Fold a day-of-week `7` (Sunday) down to `0` when parsing the dow field.
const fn fold_dow(v: u32, dow: bool) -> u32 {
    if dow && v == 7 {
        0
    } else {
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_minute_allows_every_minute() {
        let m = parse_field("*", 0, 59, false).unwrap();
        for i in 0..=59 {
            assert!(bit(m, i));
        }
    }

    #[test]
    fn step_field_sets_every_nth_bit() {
        let m = parse_field("*/15", 0, 59, false).unwrap();
        assert!(bit(m, 0) && bit(m, 15) && bit(m, 30) && bit(m, 45));
        assert!(!bit(m, 1) && !bit(m, 16));
    }

    #[test]
    fn dow_seven_folds_to_sunday() {
        let m = parse_field("7", 0, 6, true).unwrap();
        assert!(bit(m, 0), "Sunday-as-7 sets bit 0");
    }

    #[test]
    fn range_with_step() {
        // 0-30/10 → {0,10,20,30}
        let m = parse_field("0-30/10", 0, 59, false).unwrap();
        assert!(bit(m, 0) && bit(m, 10) && bit(m, 20) && bit(m, 30));
        assert!(!bit(m, 40));
    }
}
