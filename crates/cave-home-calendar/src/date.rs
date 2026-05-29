//! A minimal civil date / date-time model — std-only, no `chrono`/`time`.
//!
//! cave-home-calendar needs to do real calendar arithmetic (leap years, month
//! lengths, weekday of a date, "the third Friday of the month") to expand
//! recurring appointments. Rather than pull in a date library, we implement the
//! small slice of the proleptic Gregorian calendar the recurrence engine needs,
//! and test it hard against known reference dates.
//!
//! # Time-zone stance (Phase 1)
//! Phase 1 treats every time as a *floating* / local civil time — there is no
//! time-zone database (`TZID`) and no UTC offset arithmetic. That is sufficient
//! for a household's own calendar where "Dentist at 3pm" means 3pm wherever the
//! house is. Full `VTIMEZONE` / `TZID` handling is deferred (see the parity
//! manifest, ADR-027).

use core::cmp::Ordering;

/// Days in a non-leap year, indexed 1..=12 (index 0 is a filler).
const MONTH_LENGTHS: [u8; 13] = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/// A day of the week. `Monday` is first to match RFC 5545's default `WKST=MO`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl Weekday {
    /// 0 = Monday .. 6 = Sunday.
    #[must_use]
    pub const fn index_from_monday(self) -> u8 {
        match self {
            Self::Monday => 0,
            Self::Tuesday => 1,
            Self::Wednesday => 2,
            Self::Thursday => 3,
            Self::Friday => 4,
            Self::Saturday => 5,
            Self::Sunday => 6,
        }
    }

    /// Build from a 0=Monday..6=Sunday index, wrapping modulo 7.
    #[must_use]
    pub const fn from_index_from_monday(i: i64) -> Self {
        match i.rem_euclid(7) {
            0 => Self::Monday,
            1 => Self::Tuesday,
            2 => Self::Wednesday,
            3 => Self::Thursday,
            4 => Self::Friday,
            5 => Self::Saturday,
            _ => Self::Sunday,
        }
    }

    /// The RFC 5545 `BYDAY` two-letter code (`MO`, `TU`, ...).
    #[must_use]
    pub const fn ical_code(self) -> &'static str {
        match self {
            Self::Monday => "MO",
            Self::Tuesday => "TU",
            Self::Wednesday => "WE",
            Self::Thursday => "TH",
            Self::Friday => "FR",
            Self::Saturday => "SA",
            Self::Sunday => "SU",
        }
    }

    /// Parse a two-letter RFC 5545 day code (case-insensitive).
    #[must_use]
    pub fn from_ical_code(code: &str) -> Option<Self> {
        match code.to_ascii_uppercase().as_str() {
            "MO" => Some(Self::Monday),
            "TU" => Some(Self::Tuesday),
            "WE" => Some(Self::Wednesday),
            "TH" => Some(Self::Thursday),
            "FR" => Some(Self::Friday),
            "SA" => Some(Self::Saturday),
            "SU" => Some(Self::Sunday),
            _ => None,
        }
    }
}

/// A civil calendar date in the proleptic Gregorian calendar.
///
/// Always valid once constructed: [`Date::new`] rejects impossible dates
/// (month 0, 31 April, 29 February in a common year, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Date {
    year: i32,
    month: u8,
    day: u8,
}

/// Why a [`Date`] or [`DateTime`] could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateError {
    /// Month was not in 1..=12.
    MonthOutOfRange,
    /// Day was 0 or exceeded the length of that month (leap-year aware).
    DayOutOfRange,
    /// An hour/minute/second component was out of range.
    TimeOutOfRange,
}

impl core::fmt::Display for DateError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MonthOutOfRange => f.write_str("month is not between 1 and 12"),
            Self::DayOutOfRange => f.write_str("day is not valid for that month"),
            Self::TimeOutOfRange => f.write_str("time of day is out of range"),
        }
    }
}

impl std::error::Error for DateError {}

/// Is `year` a Gregorian leap year?
#[must_use]
pub const fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Number of days in `month` (1..=12) of `year`, leap-year aware.
/// Returns 0 for an out-of-range month.
#[must_use]
pub const fn days_in_month(year: i32, month: u8) -> u8 {
    if month == 0 || month > 12 {
        return 0;
    }
    if month == 2 && is_leap_year(year) {
        29
    } else {
        MONTH_LENGTHS[month as usize]
    }
}

impl Date {
    /// Construct a validated date.
    ///
    /// # Errors
    /// Returns [`DateError`] if the month is out of range or the day is not
    /// valid for that month (e.g. 30 February).
    pub const fn new(year: i32, month: u8, day: u8) -> Result<Self, DateError> {
        if month == 0 || month > 12 {
            return Err(DateError::MonthOutOfRange);
        }
        let max = days_in_month(year, month);
        if day == 0 || day > max {
            return Err(DateError::DayOutOfRange);
        }
        Ok(Self { year, month, day })
    }

    #[must_use]
    pub const fn year(self) -> i32 {
        self.year
    }

    #[must_use]
    pub const fn month(self) -> u8 {
        self.month
    }

    #[must_use]
    pub const fn day(self) -> u8 {
        self.day
    }

    /// Day-of-year ordinal, 1 = 1 January.
    #[must_use]
    pub const fn ordinal(self) -> u16 {
        let mut total: u16 = 0;
        let mut m: u8 = 1;
        while m < self.month {
            total += days_in_month(self.year, m) as u16;
            m += 1;
        }
        total + self.day as u16
    }

    /// Days since the epoch 1970-01-01 (which is day 0). Negative before then.
    ///
    /// Uses the well-known civil-from-days algorithm (Howard Hinnant's
    /// `days_from_civil`), implemented here from its public description.
    #[must_use]
    pub const fn days_from_epoch(self) -> i64 {
        let y = if self.month <= 2 {
            self.year as i64 - 1
        } else {
            self.year as i64
        };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400; // [0, 399]
        let m = self.month as i64;
        let d = self.day as i64;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1; // [0, 365]
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
        era * 146_097 + doe - 719_468
    }

    /// Build a date from a days-since-1970-01-01 count (inverse of
    /// [`Date::days_from_epoch`]). Implemented from the public
    /// `civil_from_days` algorithm.
    #[must_use]
    pub const fn from_days_from_epoch(z: i64) -> Self {
        let z = z + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = z - era * 146_097; // [0, 146096]
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
        let mp = (5 * doy + 2) / 153; // [0, 11]
        let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
        let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
        let year = if m <= 2 { y + 1 } else { y };
        // Components are guaranteed in range by construction.
        Self {
            year: year as i32,
            month: m as u8,
            day: d as u8,
        }
    }

    /// Weekday of this date.
    #[must_use]
    pub const fn weekday(self) -> Weekday {
        // 1970-01-01 was a Thursday (index 3 from Monday).
        let days = self.days_from_epoch();
        Weekday::from_index_from_monday((days + 3).rem_euclid(7))
    }

    /// This date advanced by `n` days (may be negative).
    #[must_use]
    pub const fn add_days(self, n: i64) -> Self {
        Self::from_days_from_epoch(self.days_from_epoch() + n)
    }

    /// Add `months` calendar months, clamping the day to the target month's
    /// length (RFC 5545's behaviour: 31 Jan + 1 month → 28/29 Feb). `months`
    /// may be negative.
    #[must_use]
    pub const fn add_months_clamped(self, months: i64) -> Self {
        let zero_based = (self.year as i64) * 12 + (self.month as i64 - 1) + months;
        let year = zero_based.div_euclid(12) as i32;
        let month = (zero_based.rem_euclid(12) + 1) as u8;
        let max = days_in_month(year, month);
        let day = if self.day > max { max } else { self.day };
        Self { year, month, day }
    }

    /// Add `years`, clamping 29 Feb → 28 Feb in a common target year.
    #[must_use]
    pub const fn add_years_clamped(self, years: i32) -> Self {
        let year = self.year + years;
        let max = days_in_month(year, self.month);
        let day = if self.day > max { max } else { self.day };
        Self {
            year,
            month: self.month,
            day,
        }
    }
}

impl PartialOrd for Date {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Date {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.year, self.month, self.day).cmp(&(other.year, other.month, other.day))
    }
}

/// A civil date plus a wall-clock time of day. Floating / local (Phase 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DateTime {
    date: Date,
    hour: u8,
    minute: u8,
    second: u8,
}

impl DateTime {
    /// Construct a validated date-time.
    ///
    /// # Errors
    /// Returns [`DateError::TimeOutOfRange`] if any time component is out of
    /// range (`hour > 23`, `minute > 59`, `second > 60` — 60 allows a leap
    /// second per RFC 5545).
    pub const fn new(
        date: Date,
        hour: u8,
        minute: u8,
        second: u8,
    ) -> Result<Self, DateError> {
        if hour > 23 || minute > 59 || second > 60 {
            return Err(DateError::TimeOutOfRange);
        }
        Ok(Self {
            date,
            hour,
            minute,
            second,
        })
    }

    /// A date-time at midnight (00:00:00) — the canonical form for an all-day
    /// appointment's start.
    #[must_use]
    pub const fn at_midnight(date: Date) -> Self {
        Self {
            date,
            hour: 0,
            minute: 0,
            second: 0,
        }
    }

    #[must_use]
    pub const fn date(self) -> Date {
        self.date
    }

    #[must_use]
    pub const fn hour(self) -> u8 {
        self.hour
    }

    #[must_use]
    pub const fn minute(self) -> u8 {
        self.minute
    }

    #[must_use]
    pub const fn second(self) -> u8 {
        self.second
    }

    /// Seconds since 1970-01-01 00:00:00, treating the time as floating/local.
    /// Used only for ordering; not an absolute UTC instant.
    #[must_use]
    pub const fn floating_unix_seconds(self) -> i64 {
        self.date.days_from_epoch() * 86_400
            + self.hour as i64 * 3600
            + self.minute as i64 * 60
            + self.second as i64
    }

    /// Replace the date, keeping the time of day.
    #[must_use]
    pub const fn with_date(self, date: Date) -> Self {
        Self { date, ..self }
    }
}

impl PartialOrd for DateTime {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DateTime {
    fn cmp(&self, other: &Self) -> Ordering {
        self.floating_unix_seconds()
            .cmp(&other.floating_unix_seconds())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leap_year_rules() {
        assert!(is_leap_year(2000)); // divisible by 400
        assert!(!is_leap_year(1900)); // divisible by 100 not 400
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(2023));
        assert!(is_leap_year(2400));
        assert!(!is_leap_year(2100));
    }

    #[test]
    fn month_lengths_leap_aware() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2024, 1), 31);
        assert_eq!(days_in_month(2024, 4), 30);
        assert_eq!(days_in_month(2024, 12), 31);
        assert_eq!(days_in_month(2024, 0), 0);
        assert_eq!(days_in_month(2024, 13), 0);
    }

    #[test]
    fn rejects_invalid_dates() {
        assert_eq!(Date::new(2024, 0, 1), Err(DateError::MonthOutOfRange));
        assert_eq!(Date::new(2024, 13, 1), Err(DateError::MonthOutOfRange));
        assert_eq!(Date::new(2024, 4, 31), Err(DateError::DayOutOfRange));
        assert_eq!(Date::new(2023, 2, 29), Err(DateError::DayOutOfRange));
        assert_eq!(Date::new(2024, 1, 0), Err(DateError::DayOutOfRange));
        assert!(Date::new(2024, 2, 29).is_ok());
    }

    #[test]
    fn weekday_of_known_dates() {
        // Reference: 1970-01-01 was a Thursday.
        assert_eq!(Date::new(1970, 1, 1).unwrap().weekday(), Weekday::Thursday);
        // 2000-01-01 was a Saturday.
        assert_eq!(Date::new(2000, 1, 1).unwrap().weekday(), Weekday::Saturday);
        // 2026-05-29 (today, per task) was a Friday.
        assert_eq!(Date::new(2026, 5, 29).unwrap().weekday(), Weekday::Friday);
        // 1900-01-01 was a Monday (1900 not a leap year).
        assert_eq!(Date::new(1900, 1, 1).unwrap().weekday(), Weekday::Monday);
        // RFC 5545 example date 1997-09-02 was a Tuesday.
        assert_eq!(Date::new(1997, 9, 2).unwrap().weekday(), Weekday::Tuesday);
    }

    #[test]
    fn days_from_epoch_round_trips() {
        for &(y, m, d) in &[
            (1970, 1, 1),
            (1969, 12, 31),
            (2000, 2, 29),
            (2024, 2, 29),
            (2026, 5, 29),
            (1900, 3, 1),
            (2400, 12, 31),
        ] {
            let date = Date::new(y, m, d).unwrap();
            let back = Date::from_days_from_epoch(date.days_from_epoch());
            assert_eq!(date, back, "round trip failed for {y}-{m}-{d}");
        }
        assert_eq!(Date::new(1970, 1, 1).unwrap().days_from_epoch(), 0);
        assert_eq!(Date::new(1970, 1, 2).unwrap().days_from_epoch(), 1);
        assert_eq!(Date::new(1969, 12, 31).unwrap().days_from_epoch(), -1);
    }

    #[test]
    fn add_days_crosses_month_and_year() {
        let d = Date::new(2024, 2, 28).unwrap();
        assert_eq!(d.add_days(1), Date::new(2024, 2, 29).unwrap()); // leap
        assert_eq!(d.add_days(2), Date::new(2024, 3, 1).unwrap());
        let nye = Date::new(2023, 12, 31).unwrap();
        assert_eq!(nye.add_days(1), Date::new(2024, 1, 1).unwrap());
        assert_eq!(nye.add_days(-1), Date::new(2023, 12, 30).unwrap());
    }

    #[test]
    fn add_months_clamps_day() {
        let jan31 = Date::new(2024, 1, 31).unwrap();
        assert_eq!(jan31.add_months_clamped(1), Date::new(2024, 2, 29).unwrap());
        let jan31_2023 = Date::new(2023, 1, 31).unwrap();
        assert_eq!(
            jan31_2023.add_months_clamped(1),
            Date::new(2023, 2, 28).unwrap()
        );
        let dec = Date::new(2023, 12, 15).unwrap();
        assert_eq!(dec.add_months_clamped(1), Date::new(2024, 1, 15).unwrap());
        assert_eq!(dec.add_months_clamped(-1), Date::new(2023, 11, 15).unwrap());
    }

    #[test]
    fn add_years_clamps_leap_day() {
        let leap = Date::new(2024, 2, 29).unwrap();
        assert_eq!(leap.add_years_clamped(1), Date::new(2025, 2, 28).unwrap());
        assert_eq!(leap.add_years_clamped(4), Date::new(2028, 2, 29).unwrap());
    }

    #[test]
    fn ordinal_counts_day_of_year() {
        assert_eq!(Date::new(2024, 1, 1).unwrap().ordinal(), 1);
        assert_eq!(Date::new(2024, 3, 1).unwrap().ordinal(), 61); // leap: 31+29+1
        assert_eq!(Date::new(2023, 3, 1).unwrap().ordinal(), 60);
        assert_eq!(Date::new(2024, 12, 31).unwrap().ordinal(), 366);
    }

    #[test]
    fn datetime_validation_and_ordering() {
        let d = Date::new(2026, 5, 29).unwrap();
        assert_eq!(DateTime::new(d, 24, 0, 0), Err(DateError::TimeOutOfRange));
        assert_eq!(DateTime::new(d, 0, 60, 0), Err(DateError::TimeOutOfRange));
        assert!(DateTime::new(d, 23, 59, 60).is_ok()); // leap second tolerated
        let morning = DateTime::new(d, 9, 0, 0).unwrap();
        let evening = DateTime::new(d, 18, 0, 0).unwrap();
        assert!(morning < evening);
        let next_day = DateTime::at_midnight(d.add_days(1));
        assert!(evening < next_day);
    }

    #[test]
    fn weekday_code_round_trip() {
        for wd in [
            Weekday::Monday,
            Weekday::Tuesday,
            Weekday::Wednesday,
            Weekday::Thursday,
            Weekday::Friday,
            Weekday::Saturday,
            Weekday::Sunday,
        ] {
            assert_eq!(Weekday::from_ical_code(wd.ical_code()), Some(wd));
        }
        assert_eq!(Weekday::from_ical_code("mo"), Some(Weekday::Monday));
        assert_eq!(Weekday::from_ical_code("XX"), None);
    }
}
