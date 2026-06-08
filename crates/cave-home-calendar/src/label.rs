//! Grandma-friendly calendar phrasing (Charter §6.3, ADR-007, ADR-027).
//!
//! The household never sees "VEVENT", "RRULE" or "CalDAV". They see "Dentist at
//! 3pm tomorrow", "Trash day every Tuesday", "Birthday next month" — in EN, DE
//! or TR. This module turns the structured model ([`crate::event::Event`],
//! [`crate::rrule::RRule`]) into those phrases.

use crate::date::{Date, DateTime, Weekday};
use crate::event::Event;
use crate::rrule::{Freq, Limit, RRule};

/// A UI language. Charter §6.3 requires EN + DE + TR from M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
    Tr,
}

/// How a date relates to "today" — the basis for "tomorrow" / "next month".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Relative {
    Today,
    Tomorrow,
    ThisWeek,
    NextMonth,
    Later,
}

fn relate(today: Date, when: Date) -> Relative {
    let delta = when.days_from_epoch() - today.days_from_epoch();
    if delta == 0 {
        Relative::Today
    } else if delta == 1 {
        Relative::Tomorrow
    } else if (2..=7).contains(&delta) {
        Relative::ThisWeek
    } else if when.year() == today.year() && when.month() == today.month() + 1
        || (today.month() == 12 && when.year() == today.year() + 1 && when.month() == 1)
    {
        Relative::NextMonth
    } else {
        Relative::Later
    }
}

/// The localised weekday name.
#[must_use]
pub const fn weekday_name(wd: Weekday, lang: Lang) -> &'static str {
    match (wd, lang) {
        (Weekday::Monday, Lang::En) => "Monday",
        (Weekday::Monday, Lang::De) => "Montag",
        (Weekday::Monday, Lang::Tr) => "Pazartesi",
        (Weekday::Tuesday, Lang::En) => "Tuesday",
        (Weekday::Tuesday, Lang::De) => "Dienstag",
        (Weekday::Tuesday, Lang::Tr) => "Salı",
        (Weekday::Wednesday, Lang::En) => "Wednesday",
        (Weekday::Wednesday, Lang::De) => "Mittwoch",
        (Weekday::Wednesday, Lang::Tr) => "Çarşamba",
        (Weekday::Thursday, Lang::En) => "Thursday",
        (Weekday::Thursday, Lang::De) => "Donnerstag",
        (Weekday::Thursday, Lang::Tr) => "Perşembe",
        (Weekday::Friday, Lang::En) => "Friday",
        (Weekday::Friday, Lang::De) => "Freitag",
        (Weekday::Friday, Lang::Tr) => "Cuma",
        (Weekday::Saturday, Lang::En) => "Saturday",
        (Weekday::Saturday, Lang::De) => "Samstag",
        (Weekday::Saturday, Lang::Tr) => "Cumartesi",
        (Weekday::Sunday, Lang::En) => "Sunday",
        (Weekday::Sunday, Lang::De) => "Sonntag",
        (Weekday::Sunday, Lang::Tr) => "Pazar",
    }
}

/// Format a time of day as a plain "3pm" / "15 Uhr" / "15:00" style phrase.
fn time_phrase(dt: DateTime, lang: Lang) -> String {
    let (h, m) = (dt.hour(), dt.minute());
    match lang {
        Lang::En => {
            let (h12, ap) = match h {
                0 => (12, "am"),
                1..=11 => (h, "am"),
                12 => (12, "pm"),
                _ => (h - 12, "pm"),
            };
            if m == 0 {
                format!("{h12}{ap}")
            } else {
                format!("{h12}:{m:02}{ap}")
            }
        }
        Lang::De => format!("{h}:{m:02} Uhr"),
        Lang::Tr => format!("saat {h}:{m:02}"),
    }
}

/// A whole, household-readable line for a single occurrence, e.g.
/// "Dentist at 3pm tomorrow".
#[must_use]
pub fn occurrence_phrase(summary: &str, when: DateTime, all_day: bool, today: Date, lang: Lang) -> String {
    let rel = relate(today, when.date());
    let day_word: String = match (rel, lang) {
        (Relative::Today, Lang::En) => "today".into(),
        (Relative::Today, Lang::De) => "heute".into(),
        (Relative::Today, Lang::Tr) => "bugün".into(),
        (Relative::Tomorrow, Lang::En) => "tomorrow".into(),
        (Relative::Tomorrow, Lang::De) => "morgen".into(),
        (Relative::Tomorrow, Lang::Tr) => "yarın".into(),
        (Relative::ThisWeek, lang) => weekday_name(when.date().weekday(), lang).to_string(),
        (Relative::NextMonth, Lang::En) => "next month".into(),
        (Relative::NextMonth, Lang::De) => "nächsten Monat".into(),
        (Relative::NextMonth, Lang::Tr) => "gelecek ay".into(),
        (Relative::Later, _) => {
            let dt = when.date();
            format!("{}-{:02}-{:02}", dt.year(), dt.month(), dt.day())
        }
    };

    if all_day {
        match lang {
            Lang::En => format!("{summary} {day_word}"),
            Lang::De => format!("{summary} {day_word}"),
            Lang::Tr => format!("{summary} {day_word}"),
        }
    } else {
        let t = time_phrase(when, lang);
        match lang {
            Lang::En => format!("{summary} at {t} {day_word}"),
            Lang::De => format!("{summary} um {t} {day_word}"),
            Lang::Tr => format!("{day_word} {t} {summary}"),
        }
    }
}

/// A household-readable description of how an appointment repeats, e.g.
/// "every Tuesday", "every other week", "every year".
#[must_use]
pub fn recurrence_phrase(rule: &RRule, lang: Lang) -> String {
    let every = match lang {
        Lang::En => "every",
        Lang::De => "jede",
        Lang::Tr => "her",
    };

    // A weekly rule with a single weekday reads best as "every Tuesday".
    if rule.freq == Freq::Weekly && rule.interval == 1 && rule.by_day.len() == 1 {
        let wd = weekday_name(rule.by_day[0].weekday, lang);
        return match lang {
            Lang::En => format!("every {wd}"),
            Lang::De => format!("jeden {wd}"),
            Lang::Tr => format!("her {wd}"),
        };
    }

    let unit = match (rule.freq, lang) {
        (Freq::Daily, Lang::En) => "day",
        (Freq::Daily, Lang::De) => "Tag",
        (Freq::Daily, Lang::Tr) => "gün",
        (Freq::Weekly, Lang::En) => "week",
        (Freq::Weekly, Lang::De) => "Woche",
        (Freq::Weekly, Lang::Tr) => "hafta",
        (Freq::Monthly, Lang::En) => "month",
        (Freq::Monthly, Lang::De) => "Monat",
        (Freq::Monthly, Lang::Tr) => "ay",
        (Freq::Yearly, Lang::En) => "year",
        (Freq::Yearly, Lang::De) => "Jahr",
        (Freq::Yearly, Lang::Tr) => "yıl",
    };

    let base = if rule.interval == 1 {
        match lang {
            Lang::En => format!("every {unit}"),
            Lang::De => format!("jeden {unit}"),
            Lang::Tr => format!("her {unit}"),
        }
    } else {
        let n = rule.interval;
        match lang {
            Lang::En => format!("every {n} {unit}s"),
            Lang::De => format!("alle {n} {unit}"),
            Lang::Tr => format!("her {n} {unit}da"),
        }
    };
    let _ = every;

    // Append a count/until note if present, kept jargon-free.
    match rule.limit {
        Limit::Count(c) => match lang {
            Lang::En => format!("{base} ({c} times)"),
            Lang::De => format!("{base} ({c} Mal)"),
            Lang::Tr => format!("{base} ({c} kez)"),
        },
        Limit::Until(d) => {
            let until = format!("{}-{:02}-{:02}", d.year(), d.month(), d.day());
            match lang {
                Lang::En => format!("{base} until {until}"),
                Lang::De => format!("{base} bis {until}"),
                Lang::Tr => format!("{until} tarihine kadar {base}"),
            }
        }
        Limit::Forever => base,
    }
}

/// A one-line summary for an event: its next occurrence plus, if it repeats,
/// the recurrence phrase — the line a Portal tile shows. `today` anchors the
/// relative wording.
#[must_use]
pub fn event_headline(event: &Event, today: Date, lang: Lang) -> String {
    match &event.rrule {
        // For a repeating event the recurrence phrase is the headline
        // ("Trash day — every Tuesday"); the single-occurrence wording is not
        // used, to avoid a misleading "tomorrow" on a series.
        Some(rule) => format!("{} — {}", event.summary, recurrence_phrase(rule, lang)),
        None => occurrence_phrase(&event.summary, event.start, event.all_day, today, lang),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date::Date;
    use crate::ical::parse::parse_vcalendar;

    fn d(y: i32, m: u8, day: u8) -> Date {
        Date::new(y, m, day).unwrap()
    }

    fn dt(y: i32, m: u8, day: u8, h: u8, min: u8) -> DateTime {
        DateTime::new(d(y, m, day), h, min, 0).unwrap()
    }

    #[test]
    fn dentist_at_3pm_tomorrow_en() {
        let today = d(2026, 5, 29);
        let when = dt(2026, 5, 30, 15, 0);
        assert_eq!(
            occurrence_phrase("Dentist", when, false, today, Lang::En),
            "Dentist at 3pm tomorrow"
        );
    }

    #[test]
    fn occurrence_phrase_all_three_languages() {
        let today = d(2026, 5, 29);
        let when = dt(2026, 5, 30, 15, 30);
        assert_eq!(
            occurrence_phrase("Dentist", when, false, today, Lang::De),
            "Dentist um 15:30 Uhr morgen"
        );
        let tr = occurrence_phrase("Dentist", when, false, today, Lang::Tr);
        assert!(tr.contains("yarın"));
        assert!(tr.contains("Dentist"));
    }

    #[test]
    fn all_day_birthday_next_month() {
        let today = d(2026, 5, 29);
        let when = DateTime::at_midnight(d(2026, 6, 15));
        assert_eq!(
            occurrence_phrase("Birthday", when, true, today, Lang::En),
            "Birthday next month"
        );
    }

    #[test]
    fn trash_day_every_tuesday() {
        let r = RRule::parse("FREQ=WEEKLY;BYDAY=TU").unwrap();
        assert_eq!(recurrence_phrase(&r, Lang::En), "every Tuesday");
        assert_eq!(recurrence_phrase(&r, Lang::De), "jeden Dienstag");
        assert_eq!(recurrence_phrase(&r, Lang::Tr), "her Salı");
    }

    #[test]
    fn every_other_week_and_yearly() {
        let biweekly = RRule::parse("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE").unwrap();
        assert_eq!(recurrence_phrase(&biweekly, Lang::En), "every 2 weeks");
        let yearly = RRule::parse("FREQ=YEARLY").unwrap();
        assert_eq!(recurrence_phrase(&yearly, Lang::En), "every year");
        assert_eq!(recurrence_phrase(&yearly, Lang::Tr), "her yıl");
    }

    #[test]
    fn recurrence_with_count_and_until() {
        let counted = RRule::parse("FREQ=DAILY;COUNT=5").unwrap();
        assert_eq!(recurrence_phrase(&counted, Lang::En), "every day (5 times)");
        let until = RRule::parse("FREQ=WEEKLY;UNTIL=20261231").unwrap();
        assert!(recurrence_phrase(&until, Lang::En).contains("until 2026-12-31"));
    }

    #[test]
    fn event_headline_uses_recurrence_for_repeating() {
        let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:trash\nSUMMARY:Trash day\n\
            DTSTART;VALUE=DATE:20260602\nRRULE:FREQ=WEEKLY;BYDAY=TU\nEND:VEVENT\nEND:VCALENDAR\n";
        let ev = Event::from_vevent(&parse_vcalendar(ics).unwrap()[0]).unwrap();
        let line = event_headline(&ev, d(2026, 5, 29), Lang::En);
        assert_eq!(line, "Trash day — every Tuesday");
    }

    #[test]
    fn ui_strings_carry_no_implementation_jargon() {
        // Charter §6.3 / ADR-027: the UI must never surface protocol terms.
        const BANNED: &[&str] = &[
            "VEVENT", "VCALENDAR", "RRULE", "CalDAV", "CardDAV", "PROPFIND",
            "iCalendar", "FREQ", "BYDAY", "DTSTART", "MQTT", "entity_id",
            "RFC", "UID",
        ];
        let today = d(2026, 5, 29);
        let langs = [Lang::En, Lang::De, Lang::Tr];

        // Occurrence + recurrence phrasing across a spread of rules and times.
        let rules = [
            "FREQ=WEEKLY;BYDAY=TU",
            "FREQ=DAILY;INTERVAL=3;COUNT=4",
            "FREQ=MONTHLY;BYDAY=1FR",
            "FREQ=YEARLY;UNTIL=20301231",
        ];
        for lang in langs {
            let phr = occurrence_phrase("Dentist", dt(2026, 5, 30, 15, 0), false, today, lang);
            for b in BANNED {
                assert!(!phr.contains(b), "occurrence phrase leaks {b:?}: {phr}");
            }
            for spec in &rules {
                let r = RRule::parse(spec).unwrap();
                let rp = recurrence_phrase(&r, lang);
                for b in BANNED {
                    assert!(!rp.contains(b), "recurrence phrase leaks {b:?}: {rp}");
                }
            }
        }
    }
}
