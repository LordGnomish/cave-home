//! `cave-home-calendar` — the household calendar engine (ADR-027, ROADMAP M10).
//!
//! This crate is the **pure-logic core** of cave-home's family calendar: it
//! reads iCalendar appointments, understands how they repeat, and answers
//! "what's coming up?" — all in plain household language (EN / DE / TR), never
//! protocol jargon.
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here — std-only, no external crates, no clock
//! and no network:
//! - [`date`] — a small civil date / date-time model (leap years, month
//!   lengths, weekday-of-date, calendar arithmetic) so the recurrence engine
//!   has correct calendar math without pulling in a date library.
//! - [`ical`] — iCalendar (RFC 5545) reading: line unfolding, content-line
//!   property/parameter parsing, and the `DATE` / `DATE-TIME` value forms.
//! - [`event`] — the [`Event`] appointment model built from a `VEVENT`
//!   (start, end / duration, all-day, recurrence, exclusions).
//! - [`rrule`] — the recurrence engine: `FREQ` DAILY/WEEKLY/MONTHLY/YEARLY
//!   with `INTERVAL`, `COUNT`, `UNTIL`, `BYDAY`, `BYMONTHDAY`, `BYMONTH`,
//!   `WKST`, expanded into concrete occurrences within a date window with
//!   `EXDATE` exclusions applied.
//! - [`agenda`] — agenda queries: chronological occurrences over a window and
//!   "the next occurrence after a moment".
//! - [`label`] — grandma-friendly phrasing ("Dentist at 3pm tomorrow", "Trash
//!   day every Tuesday", "Birthday next month") in EN / DE / TR.
//!
//! # Clean-room provenance
//!
//! The iCalendar parsing and recurrence logic are implemented **from the public
//! RFCs** (RFC 5545 iCalendar, RFC 4791 CalDAV). Radicale (the GPL-3.0 CalDAV
//! server referenced by ADR-027) was **not** read or ported (Charter §6.1).
//!
//! # Deferred (Phase 1b)
//!
//! The CalDAV *server* surface (RFC 4791 HTTP `REPORT` / `PROPFIND` / `PUT`,
//! sync-tokens / etags, storage), the full time-zone database (`TZID` /
//! `VTIMEZONE`), and `VTODO` / `VJOURNAL` handling are network/storage/
//! TZ-data-bound and are enumerated in `parity.manifest.toml` `[[unmapped]]`
//! with an ADR-027 disposition. Phase 1 treats times as floating/local.
//!
//! # Example
//!
//! ```
//! use cave_home_calendar::{parse_vcalendar, Event, agenda, Lang, event_headline};
//! use cave_home_calendar::date::Date;
//!
//! // A weekly "trash day every Tuesday", starting Tue 2 June 2026.
//! let ics = "BEGIN:VCALENDAR\r\n\
//!     BEGIN:VEVENT\r\n\
//!     UID:trash@cave\r\n\
//!     SUMMARY:Trash day\r\n\
//!     DTSTART;VALUE=DATE:20260602\r\n\
//!     RRULE:FREQ=WEEKLY;BYDAY=TU\r\n\
//!     END:VEVENT\r\n\
//!     END:VCALENDAR\r\n";
//!
//! let blocks = parse_vcalendar(ics).expect("valid calendar");
//! let event = Event::from_vevent(&blocks[0]).expect("valid event");
//!
//! // What falls in the first half of June 2026?
//! let from = Date::new(2026, 6, 1).unwrap();
//! let to = Date::new(2026, 6, 15).unwrap();
//! let week = agenda(&[event.clone()], from, to);
//! assert_eq!(week.len(), 2); // Tue Jun 2 and Tue Jun 9
//!
//! // The Portal tile line, in plain language.
//! let today = Date::new(2026, 5, 29).unwrap();
//! assert_eq!(event_headline(&event, today, Lang::En), "Trash day — every Tuesday");
//! ```

pub mod agenda;
pub mod date;
pub mod event;
pub mod ical;
pub mod label;
pub mod rrule;

pub use agenda::{agenda, next_occurrence, Occurrence};
pub use date::{Date, DateError, DateTime, Weekday};
pub use event::{parse_duration, Duration, Event};
pub use ical::parse::{parse_date_value, parse_vcalendar, ContentLine, DateValue, VEvent};
pub use ical::IcalError;
pub use label::{event_headline, occurrence_phrase, recurrence_phrase, weekday_name, Lang};
pub use rrule::{ByDay, Freq, Limit, RRule};
