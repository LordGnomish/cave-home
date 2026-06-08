//! Agenda queries — turning a set of appointments into "what's coming up".
//!
//! [`agenda`] expands every event (recurring or not) across a date window and
//! returns the concrete [`Occurrence`]s in chronological order — the list a
//! Portal tile or a voice reply ("here's your week") is built from.
//! [`next_occurrence`] answers "when is the next bin/dentist/birthday after
//! today?".

use crate::date::{Date, DateTime};
use crate::event::Event;

/// One concrete firing of an event on a specific date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    /// The originating event's `UID`.
    pub uid: String,
    /// What the household sees.
    pub summary: String,
    /// The start moment on this occurrence's date (time-of-day from the event).
    pub start: DateTime,
    /// `true` for whole-day appointments.
    pub all_day: bool,
}

impl Occurrence {
    /// The civil date this occurrence falls on.
    #[must_use]
    pub const fn date(&self) -> Date {
        self.start.date()
    }
}

/// Expand `events` over `[from, to]` (inclusive) into a chronologically sorted
/// list of occurrences. Each event's recurrence and `EXDATE` exclusions are
/// applied; the time-of-day is carried from the event's start.
#[must_use]
pub fn agenda(events: &[Event], from: Date, to: Date) -> Vec<Occurrence> {
    let mut out = Vec::new();
    for event in events {
        for date in event.occurrence_dates(from, to) {
            out.push(Occurrence {
                uid: event.uid.clone(),
                summary: event.summary.clone(),
                start: event.start.with_date(date),
                all_day: event.all_day,
            });
        }
    }
    out.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.uid.cmp(&b.uid)));
    out
}

/// The next occurrence of any event strictly after `after`, or `None` if none
/// is found within `horizon_days` of that moment.
///
/// `after` is a moment, so "next" respects time-of-day: an appointment later
/// the same day still counts as upcoming.
#[must_use]
pub fn next_occurrence(events: &[Event], after: DateTime, horizon_days: i64) -> Option<Occurrence> {
    let from = after.date();
    let to = from.add_days(horizon_days.max(0));
    agenda(events, from, to)
        .into_iter()
        .find(|o| o.start > after)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ical::parse::parse_vcalendar;

    fn d(y: i32, m: u8, day: u8) -> Date {
        Date::new(y, m, day).unwrap()
    }

    fn dt(y: i32, m: u8, day: u8, h: u8) -> DateTime {
        DateTime::new(d(y, m, day), h, 0, 0).unwrap()
    }

    fn events(ics: &str) -> Vec<Event> {
        parse_vcalendar(ics)
            .unwrap()
            .iter()
            .map(|ve| Event::from_vevent(ve).unwrap())
            .collect()
    }

    const SAMPLE: &str = "BEGIN:VCALENDAR\n\
        BEGIN:VEVENT\nUID:trash\nSUMMARY:Trash day\n\
        DTSTART;VALUE=DATE:20260602\nRRULE:FREQ=WEEKLY;BYDAY=TU\nEND:VEVENT\n\
        BEGIN:VEVENT\nUID:dentist\nSUMMARY:Dentist\n\
        DTSTART:20260603T150000\nDTEND:20260603T160000\nEND:VEVENT\n\
        END:VCALENDAR\n";

    #[test]
    fn agenda_is_sorted_and_merges_events() {
        let evs = events(SAMPLE);
        let week = agenda(&evs, d(2026, 6, 1), d(2026, 6, 9));
        // Tue Jun 2 (trash), Wed Jun 3 (dentist), Tue Jun 9 (trash).
        assert_eq!(week.len(), 3);
        assert_eq!(week[0].date(), d(2026, 6, 2));
        assert_eq!(week[0].summary, "Trash day");
        assert_eq!(week[1].date(), d(2026, 6, 3));
        assert_eq!(week[1].summary, "Dentist");
        assert_eq!(week[2].date(), d(2026, 6, 9));
    }

    #[test]
    fn same_day_sorts_all_day_before_timed() {
        // All-day trash (midnight) on Jun 2 should precede a timed event same day.
        let ics = "BEGIN:VCALENDAR\n\
            BEGIN:VEVENT\nUID:allday\nSUMMARY:Bin\nDTSTART;VALUE=DATE:20260602\nEND:VEVENT\n\
            BEGIN:VEVENT\nUID:timed\nSUMMARY:Call\nDTSTART:20260602T090000\nEND:VEVENT\n\
            END:VCALENDAR\n";
        let evs = events(ics);
        let day = agenda(&evs, d(2026, 6, 2), d(2026, 6, 2));
        assert_eq!(day[0].summary, "Bin");
        assert_eq!(day[1].summary, "Call");
    }

    #[test]
    fn next_occurrence_respects_time_of_day() {
        let evs = events(SAMPLE);
        // Just after midnight Jun 3: next is the dentist that afternoon.
        let next = next_occurrence(&evs, dt(2026, 6, 3, 0), 30).unwrap();
        assert_eq!(next.summary, "Dentist");
        assert_eq!(next.date(), d(2026, 6, 3));
        // After the dentist: next is the following Tuesday's trash.
        let after = next_occurrence(&evs, dt(2026, 6, 3, 17), 30).unwrap();
        assert_eq!(after.summary, "Trash day");
        assert_eq!(after.date(), d(2026, 6, 9));
    }

    #[test]
    fn next_occurrence_none_beyond_horizon() {
        let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:x\nSUMMARY:Far\n\
            DTSTART:20270101T100000\nEND:VEVENT\nEND:VCALENDAR\n";
        let evs = events(ics);
        // Only a 7-day horizon from mid-2026 — the 2027 event is out of reach.
        assert!(next_occurrence(&evs, dt(2026, 6, 1, 0), 7).is_none());
    }

    #[test]
    fn empty_window_yields_nothing() {
        let evs = events(SAMPLE);
        assert!(agenda(&evs, d(2025, 1, 1), d(2025, 1, 31)).is_empty());
    }
}
