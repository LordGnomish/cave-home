# ADR-021 — Notification stack (ntfy + gotify + Apprise)

## Status

**Accepted** — 2026-05-15, founder wholesale approval (Charter v6
expansion).

## Context

cave-home needs a self-hosted notification surface that hits the
Mobile companion app (ADR-006), email, SMS bridges, third-party
chat (Matrix / Telegram / Signal), and family-shared dashboards
— without depending on FCM / APNs in cave-home's own critical
path. Three upstreams cover the surface:

- **ntfy** — Apache-2.0, simple HTTP-based pub/sub push.
- **gotify** — MIT, self-hosted notification server.
- **Apprise** — MIT, multi-platform notification adapter
  library; one API speaks ~90 destination services.

## Decision

`cave-home-notify` — line-by-line port of all three (all
permissive).

- **ntfy** (binwiederhier/ntfy) — line-by-line (Apache-2.0)
- **gotify** (gotify/server) — line-by-line (MIT)
- **Apprise** (caronc/apprise) — line-by-line (MIT)

cave-home embeds an ntfy-class HTTP push server (the cave-home
own back-end relay, satisfying Charter §9's "no third-party push
relay" rule from ADR-006) plus the Apprise destination
multiplexer as a routing layer.

Port method: **line-by-line** (all permissive).

## Consequences

### Accepted gains
- Self-hosted push notifications hit the Mobile companion app
  without FCM / APNs in cave-home's *control plane*. (The
  OS-level final-hop notification still uses FCM / APNs at
  the device-OS level — unavoidable for a wake-the-screen
  pop-up. The cave-home back-end never sees user data flow
  through FCM beyond the wake-trigger.)
- Family-shared notifications (door opened, alarm armed, water
  leak) reach a dashboard, mobile, and a Matrix room from
  one automation action.

### Accepted costs
- Apprise's ~90-destination service support is iterative; the
  initial port covers the top ~15 (email, SMTP, Matrix,
  Telegram, Signal, ntfy-self, gotify-self, generic webhook).
- Self-hosting an ntfy server means the cave-home cluster
  needs a stable LAN-reachable endpoint; the Hybrid
  deployment (ADR-005) handles this transparently.

### Charter §6.3 / ADR-007 compliance
UI says "Aileye bildirim gönder", "Çocuk eve geldi notu", "Matrix
odasına yaz" — never "ntfy topic ARN", "Apprise URL syntax".

## Alternatives considered

- (a) Use FCM / APNs directly. Rejected — Charter §9; control
  plane must not depend on Google / Apple.
- (b) Apprise only (skip ntfy / gotify self-hosting). Rejected
  — Apprise is a *router*, not a *server*; cave-home needs a
  destination on its own LAN.

## Notes

[ASSUMPTION: OS-level final-hop FCM / APNs is acceptable under
Charter §9 because the cave-home back-end controls the message
content and metadata; FCM / APNs sees only a wake-token, never
user data. This interpretation can be revisited in a follow-on
ADR.]
