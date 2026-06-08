//! iCalendar (RFC 5545) reading — the parsing half of the calendar engine.
//!
//! Submodules:
//! - [`parse`] — content-line unfolding, property/parameter tokenising, and
//!   the value parsers for the date and recurrence forms cave-home needs.

pub mod parse;

pub use parse::{
    parse_date_value, parse_vcalendar, unfold_lines, ContentLine, IcalError,
};
