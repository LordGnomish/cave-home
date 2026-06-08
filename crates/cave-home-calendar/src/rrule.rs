//! Recurrence rules (RFC 5545 §3.3.10 `RECUR`), implemented clean-room from the
//! public RFC text.
//!
//! A [`RRule`] describes how an appointment repeats — "every other week on
//! Monday, Wednesday and Friday", "the first Friday of every month", "every
//! year". [`RRule::occurrences`] expands a starting date into the concrete
//! dates it recurs on, within a requested window.
//!
//! Supported parts (Phase 1): `FREQ` = `DAILY` / `WEEKLY` / `MONTHLY` /
//! `YEARLY`, `INTERVAL`, `COUNT`, `UNTIL`, `BYDAY` (with weekly day lists and
//! monthly/yearly ordinals like `1FR`, `-1SU`), `BYMONTHDAY`, `BYMONTH`,
//! `WKST`. Other parts (`BYSETPOS`, `BYYEARDAY`, `BYWEEKNO`, `BYHOUR`/...) are
//! deferred — see the parity manifest.

use crate::date::{days_in_month, Date, Weekday};
use crate::ical::parse::parse_date_value;
use crate::ical::IcalError;

/// How often an appointment repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

impl Freq {
    fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "DAILY" => Some(Self::Daily),
            "WEEKLY" => Some(Self::Weekly),
            "MONTHLY" => Some(Self::Monthly),
            "YEARLY" => Some(Self::Yearly),
            _ => None,
        }
    }
}

/// A `BYDAY` entry: a weekday with an optional ordinal (e.g. `1FR` = first
/// Friday, `-1SU` = last Sunday). A bare weekday has `ordinal == None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByDay {
    pub ordinal: Option<i8>,
    pub weekday: Weekday,
}

/// A bound on how a recurrence ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    /// No explicit end — recurs forever (a window must bound expansion).
    Forever,
    /// Stop after this many occurrences (inclusive of the first).
    Count(u32),
    /// Stop after this date (inclusive), per `UNTIL`.
    Until(Date),
}

/// A parsed recurrence rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RRule {
    pub freq: Freq,
    pub interval: u32,
    pub limit: Limit,
    pub by_day: Vec<ByDay>,
    pub by_month_day: Vec<i8>,
    pub by_month: Vec<u8>,
    pub week_start: Weekday,
}

impl RRule {
    /// Parse an `RRULE` value such as
    /// `FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE,FR`.
    ///
    /// # Errors
    /// Returns [`IcalError`] if `FREQ` is missing/unknown, or a numeric or date
    /// part is malformed.
    pub fn parse(value: &str) -> Result<Self, IcalError> {
        let bad = || IcalError::BadDateValue(value.to_string());
        let mut freq = None;
        let mut interval = 1u32;
        let mut limit = Limit::Forever;
        let mut by_day = Vec::new();
        let mut by_month_day = Vec::new();
        let mut by_month = Vec::new();
        let mut week_start = Weekday::Monday;

        for part in value.split(';').filter(|p| !p.is_empty()) {
            let (key, val) = part.split_once('=').ok_or_else(bad)?;
            match key.trim().to_ascii_uppercase().as_str() {
                "FREQ" => freq = Some(Freq::parse(val).ok_or_else(bad)?),
                "INTERVAL" => {
                    interval = val.trim().parse().map_err(|_| bad())?;
                    if interval == 0 {
                        return Err(bad());
                    }
                }
                "COUNT" => {
                    let c: u32 = val.trim().parse().map_err(|_| bad())?;
                    limit = Limit::Count(c);
                }
                "UNTIL" => {
                    let d = parse_date_value(val.trim())?;
                    limit = Limit::Until(d.date());
                }
                "BYDAY" => {
                    for tok in val.split(',').filter(|t| !t.is_empty()) {
                        by_day.push(parse_byday(tok).ok_or_else(bad)?);
                    }
                }
                "BYMONTHDAY" => {
                    for tok in val.split(',').filter(|t| !t.is_empty()) {
                        let n: i8 = tok.trim().parse().map_err(|_| bad())?;
                        if n == 0 || !(-31..=31).contains(&n) {
                            return Err(bad());
                        }
                        by_month_day.push(n);
                    }
                }
                "BYMONTH" => {
                    for tok in val.split(',').filter(|t| !t.is_empty()) {
                        let m: u8 = tok.trim().parse().map_err(|_| bad())?;
                        if m == 0 || m > 12 {
                            return Err(bad());
                        }
                        by_month.push(m);
                    }
                }
                "WKST" => {
                    week_start = Weekday::from_ical_code(val.trim()).ok_or_else(bad)?;
                }
                // Unsupported parts are ignored in Phase 1 rather than failing,
                // so a real-world calendar with e.g. BYSETPOS still loads (the
                // part simply has no effect — see parity manifest deferral).
                _ => {}
            }
        }

        Ok(Self {
            freq: freq.ok_or_else(bad)?,
            interval,
            limit,
            by_day,
            by_month_day,
            by_month,
            week_start,
        })
    }

    /// Expand the recurrence starting at `start` into the concrete dates it
    /// fires on within `[window_start, window_end]` inclusive.
    ///
    /// `COUNT` is evaluated over the *whole* series (counting from `start`),
    /// not just the window, so a windowed query of a `COUNT`-limited rule still
    /// reports the right occurrences. A hard internal cap bounds `Forever`
    /// rules so a missing window can never loop unboundedly.
    #[must_use]
    pub fn occurrences(&self, start: Date, window_start: Date, window_end: Date) -> Vec<Date> {
        // Safety cap on candidate periods so a Forever rule with a huge window
        // still terminates. Generous enough for decades of any frequency.
        const MAX_PERIODS: u32 = 1_000_000;

        let mut out = Vec::new();
        if window_end < window_start {
            return out;
        }

        let mut emitted: u32 = 0;
        let mut period: u32 = 0;

        loop {
            if period >= MAX_PERIODS {
                break;
            }
            let candidates = self.candidates_for_period(start, period);
            // candidates come back sorted ascending within the period.
            let mut all_past_until = candidates.is_empty();
            for date in candidates {
                if date < start {
                    continue; // never emit before DTSTART
                }
                if let Limit::Until(until) = self.limit {
                    if date > until {
                        all_past_until = true;
                        continue;
                    }
                }
                // This is a valid series member.
                emitted += 1;
                if date >= window_start && date <= window_end {
                    out.push(date);
                }
                if let Limit::Count(c) = self.limit {
                    if emitted >= c {
                        out.sort_unstable();
                        out.dedup();
                        return out;
                    }
                }
            }

            // Termination conditions per limit kind.
            match self.limit {
                Limit::Until(until) => {
                    // Once the period's base date passes UNTIL we are done.
                    if self.period_base(start, period) > until && all_past_until {
                        break;
                    }
                }
                Limit::Forever | Limit::Count(_) => {
                    // For Forever, stop once we have moved past the window end
                    // and produced nothing more inside it.
                    if self.period_base(start, period) > window_end {
                        break;
                    }
                }
            }
            period += 1;
        }

        out.sort_unstable();
        out.dedup();
        out
    }

    /// The base date of period `n` (the anchor we step `BY*` rules from).
    fn period_base(&self, start: Date, n: u32) -> Date {
        let step = i64::from(self.interval) * i64::from(n);
        match self.freq {
            Freq::Daily => start.add_days(step),
            Freq::Weekly => start.add_days(step * 7),
            Freq::Monthly => start.add_months_clamped(step),
            Freq::Yearly => start.add_years_clamped((step) as i32),
        }
    }

    /// All candidate dates produced by period `n`, before UNTIL/COUNT/window
    /// filtering. Sorted ascending.
    fn candidates_for_period(&self, start: Date, n: u32) -> Vec<Date> {
        let base = self.period_base(start, n);
        let mut dates = match self.freq {
            Freq::Daily => vec![base],
            Freq::Weekly => self.weekly_candidates(start, base),
            Freq::Monthly => self.monthly_candidates(base),
            Freq::Yearly => self.yearly_candidates(base),
        };
        // BYMONTH acts as a filter across all frequencies.
        if !self.by_month.is_empty() {
            dates.retain(|d| self.by_month.contains(&d.month()));
        }
        dates.sort_unstable();
        dates.dedup();
        dates
    }

    /// Weekly expansion: the set of weekdays in the week containing `base`.
    /// With no `BYDAY`, the single weekday of `start` is used.
    fn weekly_candidates(&self, start: Date, base: Date) -> Vec<Date> {
        // Find the first day of base's week per WKST.
        let wkst = self.week_start.index_from_monday() as i64;
        let base_idx = base.weekday().index_from_monday() as i64;
        let back = (base_idx - wkst).rem_euclid(7);
        let week_start_date = base.add_days(-back);

        let target_days: Vec<Weekday> = if self.by_day.is_empty() {
            vec![start.weekday()]
        } else {
            self.by_day.iter().map(|b| b.weekday).collect()
        };

        let mut out = Vec::new();
        for offset in 0..7 {
            let d = week_start_date.add_days(offset);
            if target_days.contains(&d.weekday()) {
                out.push(d);
            }
        }
        out
    }

    /// Monthly expansion within `base`'s month.
    fn monthly_candidates(&self, base: Date) -> Vec<Date> {
        let year = base.year();
        let month = base.month();
        let dim = days_in_month(year, month);

        let mut out = Vec::new();

        if !self.by_day.is_empty() {
            for bd in &self.by_day {
                Self::collect_byday_in_month(year, month, *bd, &mut out);
            }
        } else if !self.by_month_day.is_empty() {
            for &n in &self.by_month_day {
                if let Some(day) = resolve_monthday(n, dim) {
                    if let Ok(d) = Date::new(year, month, day) {
                        out.push(d);
                    }
                }
            }
        } else {
            // Plain monthly on the same day-of-month as the (clamped) base.
            out.push(base);
        }
        out
    }

    /// Yearly expansion within `base`'s year.
    fn yearly_candidates(&self, base: Date) -> Vec<Date> {
        let year = base.year();
        // The months this rule touches: BYMONTH if present, else base's month.
        let months: Vec<u8> = if self.by_month.is_empty() {
            vec![base.month()]
        } else {
            self.by_month.clone()
        };

        let mut out = Vec::new();
        for &month in &months {
            let dim = days_in_month(year, month);
            if !self.by_day.is_empty() {
                for bd in &self.by_day {
                    Self::collect_byday_in_month(year, month, *bd, &mut out);
                }
            } else if !self.by_month_day.is_empty() {
                for &n in &self.by_month_day {
                    if let Some(day) = resolve_monthday(n, dim) {
                        if let Ok(d) = Date::new(year, month, day) {
                            out.push(d);
                        }
                    }
                }
            } else {
                // Plain yearly: the base's day within this month (clamped).
                let day = base.day().min(dim);
                if let Ok(d) = Date::new(year, month, day) {
                    out.push(d);
                }
            }
        }
        out
    }

    /// Push the date(s) matching one `BYDAY` entry within a given month.
    fn collect_byday_in_month(year: i32, month: u8, bd: ByDay, out: &mut Vec<Date>) {
        let dim = days_in_month(year, month);
        // All days of that weekday in the month, in calendar order.
        let mut matches = Vec::new();
        for day in 1..=dim {
            if let Ok(d) = Date::new(year, month, day) {
                if d.weekday() == bd.weekday {
                    matches.push(d);
                }
            }
        }
        match bd.ordinal {
            None => out.extend(matches),
            Some(ord) if ord > 0 => {
                if let Some(d) = matches.get((ord as usize).saturating_sub(1)) {
                    out.push(*d);
                }
            }
            Some(ord) => {
                // Negative: count from the end (-1 = last).
                let from_end = (-ord) as usize;
                if from_end >= 1 && from_end <= matches.len() {
                    out.push(matches[matches.len() - from_end]);
                }
            }
        }
    }
}

/// Resolve a `BYMONTHDAY` value (positive from the 1st, negative from the end)
/// to a concrete day-of-month, or `None` if it falls outside the month.
fn resolve_monthday(n: i8, days_in_month: u8) -> Option<u8> {
    if n > 0 {
        let d = n as u8;
        (d <= days_in_month).then_some(d)
    } else {
        // -1 => last day.
        let from_end = (-n) as u8;
        (from_end <= days_in_month).then(|| days_in_month - from_end + 1)
    }
}

/// Parse a single `BYDAY` token like `MO`, `1FR`, `-1SU`, `+2WE`.
fn parse_byday(tok: &str) -> Option<ByDay> {
    let t = tok.trim();
    // Split leading sign+digits from the trailing 2-letter weekday code.
    let code_start = t.len().checked_sub(2)?;
    let (num, code) = t.split_at(code_start);
    let weekday = Weekday::from_ical_code(code)?;
    let ordinal = if num.is_empty() {
        None
    } else {
        Some(num.parse::<i8>().ok()?)
    };
    Some(ByDay { ordinal, weekday })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u8, day: u8) -> Date {
        Date::new(y, m, day).unwrap()
    }

    #[test]
    fn parses_basic_rule() {
        let r = RRule::parse("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE,FR;WKST=SU").unwrap();
        assert_eq!(r.freq, Freq::Weekly);
        assert_eq!(r.interval, 2);
        assert_eq!(r.week_start, Weekday::Sunday);
        assert_eq!(r.by_day.len(), 3);
        assert_eq!(r.by_day[0].weekday, Weekday::Monday);
        assert_eq!(r.by_day[0].ordinal, None);
    }

    #[test]
    fn parse_rejects_missing_freq_and_zero_interval() {
        assert!(RRule::parse("INTERVAL=2;BYDAY=MO").is_err());
        assert!(RRule::parse("FREQ=DAILY;INTERVAL=0").is_err());
        assert!(RRule::parse("FREQ=NONSENSE").is_err());
    }

    #[test]
    fn parses_ordinal_byday() {
        let r = RRule::parse("FREQ=MONTHLY;BYDAY=1FR").unwrap();
        assert_eq!(r.by_day[0].ordinal, Some(1));
        assert_eq!(r.by_day[0].weekday, Weekday::Friday);
        let r2 = RRule::parse("FREQ=MONTHLY;BYDAY=-1SU").unwrap();
        assert_eq!(r2.by_day[0].ordinal, Some(-1));
        assert_eq!(r2.by_day[0].weekday, Weekday::Sunday);
    }

    #[test]
    fn daily_count() {
        // RFC 5545 example: daily for 10 occurrences from 1997-09-02.
        let r = RRule::parse("FREQ=DAILY;COUNT=10").unwrap();
        let start = d(1997, 9, 2);
        let occ = r.occurrences(start, d(1997, 1, 1), d(1998, 1, 1));
        assert_eq!(occ.len(), 10);
        assert_eq!(occ.first(), Some(&d(1997, 9, 2)));
        assert_eq!(occ.last(), Some(&d(1997, 9, 11)));
    }

    #[test]
    fn daily_interval() {
        // Every other day for a week's window.
        let r = RRule::parse("FREQ=DAILY;INTERVAL=2").unwrap();
        let start = d(2026, 5, 1);
        let occ = r.occurrences(start, d(2026, 5, 1), d(2026, 5, 7));
        assert_eq!(occ, vec![d(2026, 5, 1), d(2026, 5, 3), d(2026, 5, 5), d(2026, 5, 7)]);
    }

    #[test]
    fn daily_until_inclusive() {
        let r = RRule::parse("FREQ=DAILY;UNTIL=20260505").unwrap();
        let start = d(2026, 5, 1);
        let occ = r.occurrences(start, d(2026, 5, 1), d(2026, 6, 1));
        assert_eq!(occ.len(), 5); // 1..=5 inclusive
        assert_eq!(occ.last(), Some(&d(2026, 5, 5)));
    }

    #[test]
    fn weekly_on_multiple_days_every_other_week() {
        // RFC 5545 example: every other week on Mon/Wed/Fri.
        // From Mon 1997-09-01.
        let r = RRule::parse("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE,FR;WKST=SU").unwrap();
        let start = d(1997, 9, 1); // Monday
        let occ = r.occurrences(start, d(1997, 9, 1), d(1997, 9, 30));
        // Week of Sep 1: Mon 1, Wed 3, Fri 5. Skip week of Sep 8.
        // Week of Sep 15: Mon 15, Wed 17, Fri 19. Skip. Week of 29: Mon 29.
        assert_eq!(
            occ,
            vec![
                d(1997, 9, 1),
                d(1997, 9, 3),
                d(1997, 9, 5),
                d(1997, 9, 15),
                d(1997, 9, 17),
                d(1997, 9, 19),
                d(1997, 9, 29),
            ]
        );
    }

    #[test]
    fn weekly_default_uses_start_weekday() {
        // No BYDAY: weekly on the start's own weekday (a Tuesday here).
        let r = RRule::parse("FREQ=WEEKLY").unwrap();
        let start = d(2026, 6, 2); // Tuesday
        let occ = r.occurrences(start, d(2026, 6, 1), d(2026, 6, 30));
        assert_eq!(occ, vec![d(2026, 6, 2), d(2026, 6, 9), d(2026, 6, 16), d(2026, 6, 23), d(2026, 6, 30)]);
    }

    #[test]
    fn monthly_first_friday() {
        // RFC 5545: monthly on the 1st Friday.
        let r = RRule::parse("FREQ=MONTHLY;BYDAY=1FR").unwrap();
        let start = d(2026, 1, 2); // first Friday of Jan 2026
        let occ = r.occurrences(start, d(2026, 1, 1), d(2026, 4, 30));
        assert_eq!(
            occ,
            vec![d(2026, 1, 2), d(2026, 2, 6), d(2026, 3, 6), d(2026, 4, 3)]
        );
    }

    #[test]
    fn monthly_last_sunday() {
        let r = RRule::parse("FREQ=MONTHLY;BYDAY=-1SU").unwrap();
        let start = d(2026, 1, 25); // last Sunday of Jan 2026
        let occ = r.occurrences(start, d(2026, 1, 1), d(2026, 3, 31));
        assert_eq!(occ, vec![d(2026, 1, 25), d(2026, 2, 22), d(2026, 3, 29)]);
    }

    #[test]
    fn monthly_by_month_day() {
        let r = RRule::parse("FREQ=MONTHLY;BYMONTHDAY=15").unwrap();
        let start = d(2026, 1, 15);
        let occ = r.occurrences(start, d(2026, 1, 1), d(2026, 3, 31));
        assert_eq!(occ, vec![d(2026, 1, 15), d(2026, 2, 15), d(2026, 3, 15)]);
    }

    #[test]
    fn monthly_last_day_negative_monthday() {
        let r = RRule::parse("FREQ=MONTHLY;BYMONTHDAY=-1").unwrap();
        let start = d(2024, 1, 31);
        let occ = r.occurrences(start, d(2024, 1, 1), d(2024, 3, 31));
        // Jan 31, Feb 29 (leap), Mar 31.
        assert_eq!(occ, vec![d(2024, 1, 31), d(2024, 2, 29), d(2024, 3, 31)]);
    }

    #[test]
    fn monthly_interval_clamps_day() {
        // Plain monthly from Jan 31 with no BY* clamps short months.
        let r = RRule::parse("FREQ=MONTHLY").unwrap();
        let start = d(2026, 1, 31);
        let occ = r.occurrences(start, d(2026, 1, 1), d(2026, 4, 30));
        assert_eq!(occ, vec![d(2026, 1, 31), d(2026, 2, 28), d(2026, 3, 31), d(2026, 4, 30)]);
    }

    #[test]
    fn yearly_simple() {
        // A birthday: yearly.
        let r = RRule::parse("FREQ=YEARLY;COUNT=3").unwrap();
        let start = d(2026, 5, 29);
        let occ = r.occurrences(start, d(2020, 1, 1), d(2030, 1, 1));
        assert_eq!(occ, vec![d(2026, 5, 29), d(2027, 5, 29), d(2028, 5, 29)]);
    }

    #[test]
    fn yearly_with_bymonth_and_byday() {
        // US-style: last Sunday of October, yearly (a clock-change kind of rule).
        let r = RRule::parse("FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU").unwrap();
        let start = d(2025, 10, 26);
        let occ = r.occurrences(start, d(2025, 1, 1), d(2027, 12, 31));
        assert_eq!(occ, vec![d(2025, 10, 26), d(2026, 10, 25), d(2027, 10, 31)]);
    }

    #[test]
    fn window_clips_both_ends() {
        let r = RRule::parse("FREQ=DAILY").unwrap();
        let start = d(2026, 1, 1);
        let occ = r.occurrences(start, d(2026, 1, 10), d(2026, 1, 12));
        assert_eq!(occ, vec![d(2026, 1, 10), d(2026, 1, 11), d(2026, 1, 12)]);
    }

    #[test]
    fn never_emits_before_dtstart() {
        let r = RRule::parse("FREQ=DAILY").unwrap();
        let start = d(2026, 5, 15);
        let occ = r.occurrences(start, d(2026, 5, 1), d(2026, 5, 17));
        assert_eq!(occ, vec![d(2026, 5, 15), d(2026, 5, 16), d(2026, 5, 17)]);
    }

    #[test]
    fn count_is_over_whole_series_not_window() {
        // COUNT=10 daily, but window starts after the series begins: only the
        // members that fall in the window AND within the first 10 are returned.
        let r = RRule::parse("FREQ=DAILY;COUNT=10").unwrap();
        let start = d(2026, 1, 1);
        let occ = r.occurrences(start, d(2026, 1, 5), d(2026, 1, 31));
        // Members 5..=10 of Jan are in window (Jan 5..10).
        assert_eq!(occ, vec![d(2026, 1, 5), d(2026, 1, 6), d(2026, 1, 7), d(2026, 1, 8), d(2026, 1, 9), d(2026, 1, 10)]);
    }
}
