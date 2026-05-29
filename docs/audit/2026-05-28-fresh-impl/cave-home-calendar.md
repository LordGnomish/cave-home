# Coverage matrix — cave-home-calendar

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · clean-room from RFC 5545 (iCalendar) and RFC 4791 (CalDAV).
**Verified:** 10/10 mapped symbols found in source · 63 test fns · drift: no.

## MAPPED (implemented + claimed)

| Spec capability | Source symbol | Verified |
|---|---|---|
| Proleptic Gregorian civil date model — leap years, month lengths, weekday-of-date, days-from-epoch round trip, month/year-clamped arithmetic | src/date.rs::{Date,DateTime,Weekday,is_leap_year,days_in_month} | yes |
| RFC 5545 §3.1 content-line unfolding (CRLF/LF + space/tab continuation) | src/ical/parse.rs::unfold_lines | yes |
| RFC 5545 §3.1/§3.2 content-line property + parameter parsing (quoted values, unquoted colon split) | src/ical/parse.rs::{parse_content_line,ContentLine} | yes |
| RFC 5545 §3.3 DATE / DATE-TIME value parsing (YYYYMMDD, YYYYMMDDTHHMMSS[Z]) | src/ical/parse.rs::parse_date_value | yes |
| RFC 5545 §3.6.1 VCALENDAR/VEVENT block parsing (skips nested VALARM/VTIMEZONE in Phase 1) | src/ical/parse.rs::parse_vcalendar | yes |
| RFC 5545 §3.3.6 DURATION value parsing (P/W/D/T/H/M/S, signed) | src/event.rs::parse_duration | yes |
| Appointment model — UID/SUMMARY/start, end via DTEND or DTSTART+DURATION, all-day, RRULE, EXDATE | src/event.rs::Event | yes |
| RFC 5545 §3.3.10 RECUR parse — FREQ/INTERVAL/COUNT/UNTIL/BYDAY/BYMONTHDAY/BYMONTH/WKST | src/rrule.rs::RRule::parse | yes |
| RFC 5545 §3.3.10 recurrence expansion into concrete occurrences within a window | src/rrule.rs::RRule::occurrences | yes |
| EXDATE exclusion applied over expanded occurrences | src/event.rs::Event::occurrence_dates | yes |
| Agenda query — chronologically sorted occurrences over a window + next-occurrence-after-a-moment | src/agenda.rs::{agenda,next_occurrence,Occurrence} | yes |
| Grandma-friendly EN/DE/TR phrasing — occurrence line, recurrence phrase, event headline | src/label.rs::{occurrence_phrase,recurrence_phrase,event_headline,weekday_name} | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)

| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| CalDAV server HTTP methods (RFC 4791 REPORT / PROPFIND / PUT) | phase-1b | ADR-027 / ROADMAP M10: the CalDAV server surface (calendar-query / calendar-multiget REPORT, PROPFIND, resource PUT/DELETE) is the HTTP front-end over this engine. Network/HTTP-bound; consumes the parsing + recurrence brain unchanged. Clean-room from RFC 4791, not from Radicale. |
| Sync-token + ETag change tracking | phase-1b | ADR-027 / ROADMAP M10: WebDAV sync-collection sync-tokens and per-resource ETags let clients sync incrementally. State/storage-bound; layered on the server scope above. Clean-room from RFC 6578 + RFC 4791. |
| Calendar storage backend (collection + resource persistence) | phase-1b | ADR-027 / ROADMAP M10: durable storage of calendars/resources on disk. Storage/IO-bound; the engine here is storage-agnostic (it parses and reasons over in-memory text). No new calendar logic. |
| Time-zone database — TZID / VTIMEZONE resolution | phase-1b | ADR-027: Phase 1 deliberately treats all times as floating/local (documented in src/date.rs and lib.rs). Full TZID resolution against a VTIMEZONE / IANA tz database is TZ-data-bound and a Phase-1b refinement; the date model already separates civil date from any offset. |
| VTODO / VJOURNAL component handling | phase-1b | ADR-027: Phase 1 ships the VEVENT (appointment) path — the household headline value. VTODO (to-dos) and VJOURNAL (notes) reuse the same content-line + recurrence plumbing with additional component properties; deferred to Phase 1b. |
| Recurrence parts beyond Phase 1 (BYSETPOS / BYYEARDAY / BYWEEKNO / BYHOUR / BYMINUTE / BYSECOND / RDATE) | phase-2 | ADR-027: the common household rules (daily/weekly/monthly/yearly with INTERVAL/COUNT/UNTIL/BYDAY/BYMONTHDAY/BYMONTH/WKST) are implemented. The rarer RECUR parts are parsed-but-ignored today and become a Phase-2 completion of the expansion engine; same RRULE plumbing, additional filters. |
| CardDAV / vCard (RFC 6352 / RFC 6350) contacts | phase-2 | ADR-027: shared contacts are a sibling PIM surface to the calendar. Out of the Phase-1 calendar-engine scope; a separate clean-room reimplementation from RFC 6352/6350 when the contacts pillar lands. |
| Radicale GPL source reuse (intentional exclusion) | permanent | Charter §6.1 / ADR-027: the upstream Radicale server is GPL-3.0 and may not be read or ported. This crate is clean-room from the public RFCs only — this gap is intentional and permanent. |
| Pre-RFC-5545 (vCalendar 1.0) / legacy iCalendar compatibility | permanent | Charter §8 no-backcompat + §7 always-latest: cave-home targets current RFC 5545 iCalendar only; no historical vCalendar 1.0 compatibility mode. |
| 32-bit ARM / pre-Linux 7.1 kernels | permanent | Charter §6.2 / ADR-003 — Linux 7.1+ only. |

## Drift notes

None — every claimed symbol exists in source. All 10 mapped spec items verified at src/ locations. All 7 unmapped items carry ADR-027 or Charter disposition (phase-1b: 5 items, phase-2: 2 items, permanent: 3 items). The adr_justified_ratio=1.00 declaration is supported by full justification on each gap.
