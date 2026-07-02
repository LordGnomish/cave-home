// SPDX-License-Identifier: Apache-2.0
//! Minimal UTC civil-time conversion: epoch-seconds → broken-down date/time and
//! back, plus the day/month rollover helpers the cron search needs.
//!
//! Uses Howard Hinnant's well-known `days_from_civil` / `civil_from_days`
//! algorithms (public-domain, proleptic Gregorian, valid for the whole epoch
//! range this crate sees). No external date crate, no clock read — pure
//! integer arithmetic over a caller-supplied epoch second.

/// A UTC date/time broken into its components, with the weekday precomputed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Civil {
    /// Proleptic Gregorian year (e.g. 2021).
    pub year: i64,
    /// Month, 1-12.
    pub month: u8,
    /// Day of month, 1-31.
    pub day: u8,
    /// Hour, 0-23.
    pub hour: u8,
    /// Minute, 0-59.
    pub minute: u8,
    /// Second, 0-59.
    pub second: u8,
    /// Day of week, 0 = Sunday .. 6 = Saturday.
    pub weekday: u8,
}

impl Civil {
    /// Break `epoch` (seconds since 1970-01-01T00:00:00Z) into UTC components.
    #[must_use]
    pub fn from_epoch(epoch: i64) -> Self {
        let days = epoch.div_euclid(86_400);
        let secs_of_day = epoch.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        // 1970-01-01 was a Thursday (weekday 4). weekday = (days + 4) mod 7.
        // Each `try_from` operand is provably in `0..=23`/`0..=59`/`0..=6`, so
        // the fallback is never taken; it keeps the conversion lint-clean.
        let weekday = u8::try_from((days.rem_euclid(7) + 4).rem_euclid(7)).unwrap_or(0);
        Self {
            year,
            month,
            day,
            hour: u8::try_from(secs_of_day / 3600).unwrap_or(0),
            minute: u8::try_from((secs_of_day % 3600) / 60).unwrap_or(0),
            second: u8::try_from(secs_of_day % 60).unwrap_or(0),
            weekday,
        }
    }

    /// The epoch second of this date at 00:00:00 (start of its day).
    #[must_use]
    pub fn start_of_day_epoch(&self) -> i64 {
        days_from_civil(self.year, self.month, self.day) * 86_400
    }
}

/// Epoch second of 00:00:00 on the day **after** `dt`.
#[must_use]
pub fn start_of_next_day(dt: &Civil) -> i64 {
    dt.start_of_day_epoch() + 86_400
}

/// Epoch second of 00:00:00 on the 1st of the month **after** `dt`'s.
#[must_use]
pub fn start_of_next_month(dt: &Civil) -> i64 {
    let (y, m) = if dt.month == 12 { (dt.year + 1, 1) } else { (dt.year, dt.month + 1) };
    days_from_civil(y, m, 1) * 86_400
}

/// Days since 1970-01-01 for a proleptic-Gregorian `(year, month, day)`
/// (Hinnant `days_from_civil`).
#[must_use]
fn days_from_civil(year: i64, month: u8, day: u8) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let m = i64::from(month);
    let d = i64::from(day);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// `(year, month, day)` for a day count since 1970-01-01 (Hinnant
/// `civil_from_days`).
#[must_use]
fn civil_from_days(days: i64) -> (i64, u8, u8) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    // `d` is provably in `1..=31` and `m` in `1..=12`, so the fallbacks never
    // fire; they keep the narrowing conversions lint-clean.
    let d = u8::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1); // [1, 31]
    let m = u8::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1); // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_zero_is_thursday_1970() {
        let c = Civil::from_epoch(0);
        assert_eq!((c.year, c.month, c.day), (1970, 1, 1));
        assert_eq!((c.hour, c.minute, c.second), (0, 0, 0));
        assert_eq!(c.weekday, 4, "1970-01-01 is a Thursday");
    }

    #[test]
    fn known_friday() {
        // 2021-01-01T00:00:00Z is 1_609_459_200 and a Friday (weekday 5).
        let c = Civil::from_epoch(1_609_459_200);
        assert_eq!((c.year, c.month, c.day), (2021, 1, 1));
        assert_eq!(c.weekday, 5);
    }

    #[test]
    fn roundtrip_arbitrary_instant() {
        // 2024-02-29T13:37:45Z
        let epoch = 1_709_213_865;
        let c = Civil::from_epoch(epoch);
        assert_eq!((c.year, c.month, c.day), (2024, 2, 29));
        assert_eq!((c.hour, c.minute, c.second), (13, 37, 45));
        assert_eq!(c.start_of_day_epoch(), 1_709_164_800);
    }

    #[test]
    fn next_month_rolls_over_year() {
        let dec = Civil::from_epoch(1_640_995_200 - 86_400); // 2021-12-31
        assert_eq!(start_of_next_month(&dec), 1_640_995_200); // 2022-01-01
    }
}
