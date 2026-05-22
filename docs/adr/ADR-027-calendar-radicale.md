# ADR-027 — Calendar / PIM (Radicale clean-room)

## Status

**Accepted** — 2026-05-15, founder wholesale approval (Charter v6
expansion).

## Context

Family-shared calendars (CalDAV) + contacts (CardDAV) routinely
live in HA-orbit deployments as a Radicale instance. HA's
`caldav` integration already consumes external CalDAV servers;
cave-home brings the *server* in-process.

Radicale upstream is GPL-3.0, so the clean-room mandate
(Charter §6.1 / ADR-002) applies. The CalDAV / CardDAV
protocols themselves are RFCs (4791, 6352) — publicly
specified and freely reimplementable.

## Decision

`cave-home-calendar` — **clean-room** Rust reimplementation of a
CalDAV + CardDAV server from RFC 4791 + RFC 6352 + RFC 5545
(iCalendar) + RFC 6350 (vCard). Contributors must **NOT** read
Radicale source.

HA `caldav` integration → line-by-line port, consumes the
cave-home-calendar server as the local CalDAV target.

Port method:
- Server surface: **clean-room** (Charter §6.1)
- HA caldav-client integration: line-by-line (Apache-2.0)

## Consequences

### Accepted gains
- Family-shared calendars + contacts inside cave-home, never
  through Google / iCloud.
- Calendar entries become automation triggers ("school
  closure tomorrow → no morning routine alarm").

### Accepted costs
- CalDAV is moderately complex (free-busy, recurring events,
  attendees, scheduling). Clean-room from RFC is substantive
  work.
- Contributor recusal: anyone who has read Radicale source is
  barred from the server-surface portion (Charter §6.1).

### Charter §6.3 / ADR-007 compliance
UI says "Aile takvimi", "Çocuk randevuları", "Hatırlatma
ekle" — never "CalDAV PROPFIND", "iCalendar RRULE syntax".

## Alternatives considered

- (a) External CalDAV server (consume only). Rejected —
  Charter §1 "one binary" mandate; cave-home should not
  require a second always-on calendar process.
- (b) Vendor CalDAV (Google Calendar API). Rejected — Charter
  §9 cloud-free posture.

## Notes

[ASSUMPTION: CalDAV / CardDAV RFCs are sufficient
documentation for a clean-room port without reading
Radicale's Python source. Reviewers verify PRs cite RFC
sections, not Radicale source files.]
