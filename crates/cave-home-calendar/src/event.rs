//! The appointment ([`Event`]) model — the household-level view of a `VEVENT`.
//!
//! An [`Event`] is built by parsing a `VEVENT` ([`Event::from_vevent`]) or
//! directly in code. It captures only what Phase 1 reasons about: who/what
//! (`uid`, `summary`), when (`start`, plus an `end` derived from `DTEND` or
//! `DURATION`), whether it is an all-day appointment, and how it repeats
//! (`rrule` + `exdates`).

use crate::date::{Date, DateTime};
use crate::ical::parse::{parse_date_value, DateValue, VEvent};
use crate::ical::IcalError;
use crate::rrule::RRule;

/// A calendar appointment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Stable identifier (`UID`).
    pub uid: String,
    /// Human title (`SUMMARY`) — what the household sees ("Dentist").
    pub summary: String,
    /// When the appointment starts.
    pub start: DateTime,
    /// When it ends. Derived from `DTEND`, or `DTSTART + DURATION`, or — for an
    /// all-day appointment with neither — the end of the start day.
    pub end: DateTime,
    /// `true` if this was a whole-day appointment (`VALUE=DATE`).
    pub all_day: bool,
    /// The recurrence rule, if the appointment repeats.
    pub rrule: Option<RRule>,
    /// Dates explicitly excluded from the recurrence (`EXDATE`).
    pub exdates: Vec<Date>,
}

/// A length of time, parsed from an RFC 5545 `DURATION` (`PT1H`, `P1D`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Duration {
    pub days: i64,
    pub seconds: i64,
}

impl Duration {
    /// Total seconds (days folded in).
    #[must_use]
    pub const fn total_seconds(self) -> i64 {
        self.days * 86_400 + self.seconds
    }
}

/// Parse an RFC 5545 `DURATION` value, e.g. `P1D`, `PT1H30M`, `P1W`, `PT45M`.
/// Negative durations (leading `-`) are supported.
///
/// # Errors
/// Returns [`IcalError::BadDateValue`] if the form is not a valid duration.
pub fn parse_duration(raw: &str) -> Result<Duration, IcalError> {
    let bad = || IcalError::BadDateValue(raw.to_string());
    let mut s = raw.trim();
    let mut sign = 1i64;
    if let Some(rest) = s.strip_prefix('-') {
        sign = -1;
        s = rest;
    } else if let Some(rest) = s.strip_prefix('+') {
        s = rest;
    }
    let s = s.strip_prefix('P').ok_or_else(bad)?;

    let mut days = 0i64;
    let mut seconds = 0i64;
    let mut in_time = false;
    let mut num = String::new();
    let mut saw_any = false;

    for ch in s.chars() {
        match ch {
            'T' => in_time = true,
            '0'..='9' => num.push(ch),
            unit => {
                let n: i64 = num.parse().map_err(|_| bad())?;
                num.clear();
                saw_any = true;
                match (in_time, unit) {
                    (false, 'W') => days += n * 7,
                    (false, 'D') => days += n,
                    (true, 'H') => seconds += n * 3600,
                    (true, 'M') => seconds += n * 60,
                    (true, 'S') => seconds += n,
                    _ => return Err(bad()),
                }
            }
        }
    }
    if !num.is_empty() || !saw_any {
        return Err(bad());
    }
    Ok(Duration {
        days: sign * days,
        seconds: sign * seconds,
    })
}

/// Advance a [`DateTime`] by a [`Duration`] (Phase 1: floating local time).
#[must_use]
fn add_duration(dt: DateTime, dur: Duration) -> DateTime {
    let total = dt.floating_unix_seconds() + dur.total_seconds();
    let days = total.div_euclid(86_400);
    let rem = total.rem_euclid(86_400);
    let date = Date::from_days_from_epoch(days);
    let hour = (rem / 3600) as u8;
    let minute = ((rem % 3600) / 60) as u8;
    let second = (rem % 60) as u8;
    // Components are guaranteed in range by the modular arithmetic; fall back
    // to the start of day on the impossible error path rather than panicking.
    DateTime::new(date, hour, minute, second).unwrap_or_else(|_| DateTime::at_midnight(date))
}

impl Event {
    /// Build an [`Event`] from a parsed [`VEvent`] block.
    ///
    /// # Errors
    /// Returns [`IcalError`] if `UID` or `DTSTART` is missing, or any date,
    /// duration or recurrence value is malformed.
    pub fn from_vevent(ve: &VEvent) -> Result<Self, IcalError> {
        let uid = ve
            .get("UID")
            .map(|l| l.value.clone())
            .ok_or_else(|| IcalError::BadDateValue("missing UID".into()))?;
        let summary = ve.get("SUMMARY").map(|l| l.value.clone()).unwrap_or_default();

        let dtstart_line = ve
            .get("DTSTART")
            .ok_or_else(|| IcalError::BadDateValue("missing DTSTART".into()))?;
        let start_val = parse_date_value(&dtstart_line.value)?;
        let all_day = start_val.is_all_day()
            || dtstart_line
                .param("VALUE")
                .is_some_and(|v| v.eq_ignore_ascii_case("DATE"));
        let start = start_val.as_datetime();

        // End: prefer DTEND, else DURATION, else fall back.
        let end = if let Some(dtend) = ve.get("DTEND") {
            parse_date_value(&dtend.value)?.as_datetime()
        } else if let Some(dur_line) = ve.get("DURATION") {
            let dur = parse_duration(&dur_line.value)?;
            add_duration(start, dur)
        } else if all_day {
            // All-day with no end: ends at the start of the next day.
            DateTime::at_midnight(start.date().add_days(1))
        } else {
            // Timed appointment with no end/duration: zero-length.
            start
        };

        let rrule = match ve.get("RRULE") {
            Some(line) => Some(RRule::parse(&line.value)?),
            None => None,
        };

        let mut exdates = Vec::new();
        for line in ve.all("EXDATE") {
            for token in line.value.split(',').filter(|t| !t.is_empty()) {
                let v: DateValue = parse_date_value(token)?;
                exdates.push(v.date());
            }
        }

        Ok(Self {
            uid,
            summary,
            start,
            end,
            all_day,
            rrule,
            exdates,
        })
    }

    /// The dates this event occurs on within `[from, to]` inclusive, with
    /// `EXDATE` exclusions applied and the result sorted/de-duplicated.
    ///
    /// A non-recurring event yields its single start date if it falls in the
    /// window.
    #[must_use]
    pub fn occurrence_dates(&self, from: Date, to: Date) -> Vec<Date> {
        let start_date = self.start.date();
        let mut dates = match &self.rrule {
            Some(rule) => rule.occurrences(start_date, from, to),
            None => {
                if start_date >= from && start_date <= to {
                    vec![start_date]
                } else {
                    Vec::new()
                }
            }
        };
        if !self.exdates.is_empty() {
            dates.retain(|d| !self.exdates.contains(d));
        }
        dates
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ical::parse::parse_vcalendar;

    fn d(y: i32, m: u8, day: u8) -> Date {
        Date::new(y, m, day).unwrap()
    }

    fn first_event(ics: &str) -> Event {
        let evs = parse_vcalendar(ics).unwrap();
        Event::from_vevent(&evs[0]).unwrap()
    }

    #[test]
    fn parses_timed_event_with_dtend() {
        let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:1\nSUMMARY:Dentist\n\
            DTSTART:20260530T150000\nDTEND:20260530T160000\nEND:VEVENT\nEND:VCALENDAR\n";
        let e = first_event(ics);
        assert_eq!(e.summary, "Dentist");
        assert!(!e.all_day);
        assert_eq!(e.start.hour(), 15);
        assert_eq!(e.end.hour(), 16);
    }

    #[test]
    fn parses_all_day_event() {
        let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:2\nSUMMARY:Holiday\n\
            DTSTART;VALUE=DATE:20261225\nEND:VEVENT\nEND:VCALENDAR\n";
        let e = first_event(ics);
        assert!(e.all_day);
        assert_eq!(e.start.date(), d(2026, 12, 25));
        // No end: rolls to the next midnight.
        assert_eq!(e.end.date(), d(2026, 12, 26));
    }

    #[test]
    fn parses_duration_forms() {
        assert_eq!(parse_duration("PT1H").unwrap().total_seconds(), 3600);
        assert_eq!(parse_duration("PT1H30M").unwrap().total_seconds(), 5400);
        assert_eq!(parse_duration("P1D").unwrap().total_seconds(), 86_400);
        assert_eq!(parse_duration("P1W").unwrap().total_seconds(), 7 * 86_400);
        assert_eq!(parse_duration("PT45M").unwrap().total_seconds(), 2700);
        assert_eq!(parse_duration("-PT1H").unwrap().total_seconds(), -3600);
        assert!(parse_duration("1H").is_err());
        assert!(parse_duration("P").is_err());
    }

    #[test]
    fn derives_end_from_duration() {
        let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:3\nSUMMARY:Call\n\
            DTSTART:20260530T150000\nDURATION:PT90M\nEND:VEVENT\nEND:VCALENDAR\n";
        let e = first_event(ics);
        assert_eq!(e.end.hour(), 16);
        assert_eq!(e.end.minute(), 30);
        assert_eq!(e.end.date(), d(2026, 5, 30));
    }

    #[test]
    fn duration_crossing_midnight() {
        let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:3b\n\
            DTSTART:20260530T230000\nDURATION:PT2H\nEND:VEVENT\nEND:VCALENDAR\n";
        let e = first_event(ics);
        assert_eq!(e.end.date(), d(2026, 5, 31));
        assert_eq!(e.end.hour(), 1);
    }

    #[test]
    fn missing_uid_or_dtstart_errors() {
        let no_uid = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nSUMMARY:x\nDTSTART:20260101\nEND:VEVENT\nEND:VCALENDAR\n";
        let evs = parse_vcalendar(no_uid).unwrap();
        assert!(Event::from_vevent(&evs[0]).is_err());
        let no_start = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:1\nEND:VEVENT\nEND:VCALENDAR\n";
        let evs = parse_vcalendar(no_start).unwrap();
        assert!(Event::from_vevent(&evs[0]).is_err());
    }

    #[test]
    fn recurring_event_with_exdate() {
        // Weekly trash day every Tuesday, but skip one week (EXDATE).
        let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:trash\nSUMMARY:Trash day\n\
            DTSTART;VALUE=DATE:20260602\nRRULE:FREQ=WEEKLY;BYDAY=TU\n\
            EXDATE;VALUE=DATE:20260616\nEND:VEVENT\nEND:VCALENDAR\n";
        let e = first_event(ics);
        assert!(e.rrule.is_some());
        let occ = e.occurrence_dates(d(2026, 6, 1), d(2026, 6, 30));
        // Tuesdays in June 2026: 2, 9, 16, 23, 30 — minus excluded 16.
        assert_eq!(occ, vec![d(2026, 6, 2), d(2026, 6, 9), d(2026, 6, 23), d(2026, 6, 30)]);
    }

    #[test]
    fn non_recurring_event_in_and_out_of_window() {
        let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:once\n\
            DTSTART:20260530T150000\nEND:VEVENT\nEND:VCALENDAR\n";
        let e = first_event(ics);
        assert_eq!(e.occurrence_dates(d(2026, 5, 1), d(2026, 5, 31)), vec![d(2026, 5, 30)]);
        assert!(e.occurrence_dates(d(2026, 6, 1), d(2026, 6, 30)).is_empty());
    }

    #[test]
    fn multiple_events_in_one_calendar() {
        let ics = "BEGIN:VCALENDAR\n\
            BEGIN:VEVENT\nUID:a\nSUMMARY:One\nDTSTART:20260101T100000\nEND:VEVENT\n\
            BEGIN:VEVENT\nUID:b\nSUMMARY:Two\nDTSTART:20260102T100000\nEND:VEVENT\n\
            END:VCALENDAR\n";
        let evs = parse_vcalendar(ics).unwrap();
        assert_eq!(evs.len(), 2);
        let e2 = Event::from_vevent(&evs[1]).unwrap();
        assert_eq!(e2.summary, "Two");
    }
}
