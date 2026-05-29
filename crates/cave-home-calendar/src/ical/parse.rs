//! iCalendar content-line parsing, implemented from RFC 5545.
//!
//! This is a clean-room implementation from the public RFC 5545 text (§3.1
//! content lines + line folding, §3.2 parameters, §3.3 value types). It is the
//! parsing slice cave-home needs for Phase 1: enough to read a `VCALENDAR`
//! wrapping one or more `VEVENT`s with `DTSTART` / `DTEND` / `DURATION` /
//! `SUMMARY` / `UID` / `RRULE` / `EXDATE`.

use crate::date::{Date, DateError, DateTime};

/// Why an iCalendar text could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IcalError {
    /// A content line had no `:` separating name from value.
    MalformedContentLine(String),
    /// A property had an empty name.
    EmptyPropertyName,
    /// A date or date-time value did not match RFC 5545 forms.
    BadDateValue(String),
    /// A date component was itself out of range.
    BadDate(DateError),
    /// `BEGIN`/`END` blocks were not correctly nested or were missing.
    UnbalancedBlock(String),
    /// No `VEVENT` was found inside the calendar.
    NoEvents,
}

impl core::fmt::Display for IcalError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MalformedContentLine(l) => write!(f, "malformed content line: {l}"),
            Self::EmptyPropertyName => f.write_str("content line has an empty property name"),
            Self::BadDateValue(v) => write!(f, "unrecognised date value: {v}"),
            Self::BadDate(e) => write!(f, "invalid date: {e}"),
            Self::UnbalancedBlock(b) => write!(f, "unbalanced BEGIN/END block: {b}"),
            Self::NoEvents => f.write_str("calendar contained no appointments"),
        }
    }
}

impl std::error::Error for IcalError {}

impl From<DateError> for IcalError {
    fn from(e: DateError) -> Self {
        Self::BadDate(e)
    }
}

/// One unfolded, tokenised content line: `NAME;PARAM=VAL:VALUE`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentLine {
    /// Property name, upper-cased (e.g. `DTSTART`, `RRULE`).
    pub name: String,
    /// Parameters as `(KEY, VALUE)` pairs, key upper-cased.
    pub params: Vec<(String, String)>,
    /// The raw value text (everything after the first unquoted `:`).
    pub value: String,
}

impl ContentLine {
    /// Look up a parameter value by (case-insensitive) key.
    #[must_use]
    pub fn param(&self, key: &str) -> Option<&str> {
        let key = key.to_ascii_uppercase();
        self.params
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// Unfold RFC 5545 line folding: a CRLF (or LF) immediately followed by a space
/// or tab is a continuation of the previous line and the break + leading
/// whitespace char are removed. Robust to both `\r\n` and bare `\n`.
#[must_use]
pub fn unfold_lines(input: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in input.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix(' ').or_else(|| line.strip_prefix('\t')) {
            if let Some(last) = out.last_mut() {
                last.push_str(rest);
                continue;
            }
        }
        // Skip the trailing empty fragment produced by a final newline.
        if line.is_empty() && out.last().is_some_and(String::is_empty) {
            continue;
        }
        out.push(line.to_string());
    }
    out.retain(|l| !l.is_empty());
    out
}

/// Parse one already-unfolded content line into name / params / value.
///
/// # Errors
/// Returns [`IcalError`] if there is no `:` or the property name is empty.
pub fn parse_content_line(line: &str) -> Result<ContentLine, IcalError> {
    // Find the first ':' that is not inside a double-quoted parameter value.
    let mut in_quotes = false;
    let mut colon = None;
    for (i, ch) in line.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ':' if !in_quotes => {
                colon = Some(i);
                break;
            }
            _ => {}
        }
    }
    let colon = colon.ok_or_else(|| IcalError::MalformedContentLine(line.to_string()))?;
    let (head, value) = line.split_at(colon);
    let value = &value[1..]; // drop the ':'

    // head = NAME[;PARAM=VAL[;PARAM=VAL]...], split on unquoted ';'.
    let segments = split_unquoted(head, ';');
    let mut iter = segments.into_iter();
    let name = iter
        .next()
        .map(|s| s.trim().to_ascii_uppercase())
        .unwrap_or_default();
    if name.is_empty() {
        return Err(IcalError::EmptyPropertyName);
    }

    let mut params = Vec::new();
    for seg in iter {
        if let Some(eq) = seg.find('=') {
            let key = seg[..eq].trim().to_ascii_uppercase();
            let val = seg[eq + 1..].trim().trim_matches('"').to_string();
            params.push((key, val));
        }
    }

    Ok(ContentLine {
        name,
        params,
        value: value.to_string(),
    })
}

/// Split `s` on `sep`, ignoring separators inside double quotes.
fn split_unquoted(s: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for ch in s.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                cur.push(ch);
            }
            c if c == sep && !in_quotes => {
                out.push(core::mem::take(&mut cur));
            }
            c => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// A parsed date value: either a whole-day `DATE` or a `DATE-TIME`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateValue {
    /// `VALUE=DATE` form `YYYYMMDD` — an all-day marker.
    Day(Date),
    /// `DATE-TIME` form `YYYYMMDDTHHMMSS` (optionally trailing `Z` for UTC,
    /// which Phase 1 treats as floating; see the time-zone stance in `date`).
    Moment(DateTime),
}

impl DateValue {
    /// The civil date regardless of form.
    #[must_use]
    pub const fn date(self) -> Date {
        match self {
            Self::Day(d) => d,
            Self::Moment(dt) => dt.date(),
        }
    }

    /// As a [`DateTime`] — all-day values resolve to midnight.
    #[must_use]
    pub const fn as_datetime(self) -> DateTime {
        match self {
            Self::Day(d) => DateTime::at_midnight(d),
            Self::Moment(dt) => dt,
        }
    }

    /// Whether this was the whole-day `DATE` form.
    #[must_use]
    pub const fn is_all_day(self) -> bool {
        matches!(self, Self::Day(_))
    }
}

/// Parse an RFC 5545 `DATE` (`YYYYMMDD`) or `DATE-TIME`
/// (`YYYYMMDDTHHMMSS[Z]`) value.
///
/// # Errors
/// Returns [`IcalError`] if the string is not one of those forms or names an
/// impossible date.
pub fn parse_date_value(raw: &str) -> Result<DateValue, IcalError> {
    let s = raw.trim();
    let bad = || IcalError::BadDateValue(raw.to_string());

    let parse_ymd = |d: &str| -> Result<Date, IcalError> {
        if d.len() != 8 || !d.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad());
        }
        let year: i32 = d[0..4].parse().map_err(|_| bad())?;
        let month: u8 = d[4..6].parse().map_err(|_| bad())?;
        let day: u8 = d[6..8].parse().map_err(|_| bad())?;
        Ok(Date::new(year, month, day)?)
    };

    if let Some((datepart, timepart)) = s.split_once('T') {
        let date = parse_ymd(datepart)?;
        let t = timepart.strip_suffix('Z').unwrap_or(timepart);
        if t.len() != 6 || !t.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad());
        }
        let hour: u8 = t[0..2].parse().map_err(|_| bad())?;
        let minute: u8 = t[2..4].parse().map_err(|_| bad())?;
        let second: u8 = t[4..6].parse().map_err(|_| bad())?;
        let dt = DateTime::new(date, hour, minute, second)?;
        Ok(DateValue::Moment(dt))
    } else {
        Ok(DateValue::Day(parse_ymd(s)?))
    }
}

/// A `VEVENT` block as a flat list of content lines, plus the calendar-level
/// properties surrounding it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VEvent {
    /// The content lines that appeared between `BEGIN:VEVENT` and `END:VEVENT`.
    pub lines: Vec<ContentLine>,
}

impl VEvent {
    /// First content line with the given (upper-case) property name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&ContentLine> {
        self.lines.iter().find(|l| l.name == name)
    }

    /// All content lines with the given property name (e.g. multiple `EXDATE`).
    #[must_use]
    pub fn all(&self, name: &str) -> Vec<&ContentLine> {
        self.lines.iter().filter(|l| l.name == name).collect()
    }
}

/// Parse a `VCALENDAR` text into its constituent `VEVENT` blocks.
///
/// Nested non-VEVENT blocks (e.g. `VTIMEZONE`, `VALARM`) are skipped — Phase 1
/// only consumes events (see the parity manifest for the deferral note).
///
/// # Errors
/// Returns [`IcalError`] on malformed content lines, unbalanced `BEGIN`/`END`
/// blocks, or when the calendar contains no events.
pub fn parse_vcalendar(input: &str) -> Result<Vec<VEvent>, IcalError> {
    let lines = unfold_lines(input);
    let mut events = Vec::new();
    // Depth of nesting inside non-VEVENT blocks (VTIMEZONE/VALARM/...).
    let mut block_stack: Vec<String> = Vec::new();
    let mut current: Option<VEvent> = None;

    for line in &lines {
        let cl = parse_content_line(line)?;
        match (cl.name.as_str(), cl.value.to_ascii_uppercase().as_str()) {
            ("BEGIN", "VEVENT") => {
                if current.is_some() {
                    return Err(IcalError::UnbalancedBlock("nested VEVENT".into()));
                }
                current = Some(VEvent::default());
            }
            ("END", "VEVENT") => {
                let ev = current
                    .take()
                    .ok_or_else(|| IcalError::UnbalancedBlock("END:VEVENT".into()))?;
                events.push(ev);
            }
            ("BEGIN", other) => {
                if current.is_some() {
                    // A nested block inside a VEVENT (e.g. VALARM): track to skip.
                    block_stack.push(other.to_string());
                }
            }
            ("END", _other) => {
                block_stack.pop();
            }
            _ => {
                if let Some(ev) = current.as_mut() {
                    if block_stack.is_empty() {
                        ev.lines.push(cl);
                    }
                    // else: inside a nested VALARM etc. — skipped in Phase 1.
                }
            }
        }
    }

    if current.is_some() {
        return Err(IcalError::UnbalancedBlock("missing END:VEVENT".into()));
    }
    if events.is_empty() {
        return Err(IcalError::NoEvents);
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfolds_crlf_and_lf_folding() {
        // A SUMMARY split across two physical lines with a leading space.
        let folded = "SUMMARY:Dentist appoint\r\n ment\r\nUID:abc\r\n";
        let lines = unfold_lines(folded);
        assert_eq!(lines, vec!["SUMMARY:Dentist appointment", "UID:abc"]);
    }

    #[test]
    fn unfolds_with_tab_continuation_and_bare_lf() {
        let folded = "DESCRIPTION:line one\n\tand two\nUID:x\n";
        let lines = unfold_lines(folded);
        assert_eq!(lines, vec!["DESCRIPTION:line oneand two", "UID:x"]);
    }

    #[test]
    fn parses_name_params_value() {
        let cl = parse_content_line("DTSTART;TZID=Europe/Berlin;VALUE=DATE-TIME:20260529T150000")
            .unwrap();
        assert_eq!(cl.name, "DTSTART");
        assert_eq!(cl.param("TZID"), Some("Europe/Berlin"));
        assert_eq!(cl.param("value"), Some("DATE-TIME"));
        assert_eq!(cl.value, "20260529T150000");
    }

    #[test]
    fn quoted_param_value_keeps_colon() {
        let cl = parse_content_line("X-TEST;CN=\"Last:First\":hello").unwrap();
        assert_eq!(cl.param("CN"), Some("Last:First"));
        assert_eq!(cl.value, "hello");
    }

    #[test]
    fn rejects_line_without_colon() {
        assert!(matches!(
            parse_content_line("JUST-A-NAME"),
            Err(IcalError::MalformedContentLine(_))
        ));
    }

    #[test]
    fn parses_date_only_value() {
        let v = parse_date_value("20260529").unwrap();
        assert!(v.is_all_day());
        assert_eq!(v.date(), Date::new(2026, 5, 29).unwrap());
    }

    #[test]
    fn parses_datetime_value_with_and_without_z() {
        let local = parse_date_value("20260529T153000").unwrap();
        assert!(!local.is_all_day());
        let dt = local.as_datetime();
        assert_eq!(dt.hour(), 15);
        assert_eq!(dt.minute(), 30);
        // Trailing Z (UTC) parses to the same civil components in Phase 1.
        let z = parse_date_value("20260529T153000Z").unwrap();
        assert_eq!(z.as_datetime(), dt);
    }

    #[test]
    fn rejects_bad_date_values() {
        assert!(parse_date_value("2026-05-29").is_err());
        assert!(parse_date_value("20260229").is_err()); // 2026 not leap
        assert!(parse_date_value("20260529T9999").is_err());
    }

    #[test]
    fn parses_vcalendar_with_single_event() {
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\n\
            UID:1@cave\r\nSUMMARY:Dentist\r\nDTSTART:20260529T150000\r\n\
            END:VEVENT\r\nEND:VCALENDAR\r\n";
        let events = parse_vcalendar(ics).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].get("SUMMARY").unwrap().value, "Dentist");
        assert_eq!(events[0].get("UID").unwrap().value, "1@cave");
    }

    #[test]
    fn skips_nested_valarm_inside_event() {
        let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:1\nSUMMARY:Meeting\n\
            BEGIN:VALARM\nACTION:DISPLAY\nTRIGGER:-PT15M\nEND:VALARM\n\
            END:VEVENT\nEND:VCALENDAR\n";
        let events = parse_vcalendar(ics).unwrap();
        assert_eq!(events.len(), 1);
        // The VALARM's ACTION/TRIGGER must not leak into the event's lines.
        assert!(events[0].get("ACTION").is_none());
        assert!(events[0].get("TRIGGER").is_none());
        assert_eq!(events[0].get("SUMMARY").unwrap().value, "Meeting");
    }

    #[test]
    fn errors_on_empty_calendar() {
        let ics = "BEGIN:VCALENDAR\nVERSION:2.0\nEND:VCALENDAR\n";
        assert_eq!(parse_vcalendar(ics), Err(IcalError::NoEvents));
    }

    #[test]
    fn errors_on_unbalanced_event() {
        let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:1\nEND:VCALENDAR\n";
        assert!(matches!(
            parse_vcalendar(ics),
            Err(IcalError::UnbalancedBlock(_))
        ));
    }
}
