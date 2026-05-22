# ADR-010 — Philips Hue port + Hue-Bridge emulator

## Status

**Accepted** — 2026-05-15, finalised by Burak Tartan (founder).

Created: 2026-05-15
Supersedes: —
Superseded by: —

## Context

Philips Hue is the second-most-common smart-home brand in the
headline persona's home (Charter §2 personas 1–2), after UniFi.
Two distinct surfaces matter:

1. **The Hue Bridge as a client target.** Users with an existing
   Hue Bridge want cave-home to talk to it over the official
   Philips Hue API. The HA `hue` integration (Apache-2.0) already
   does this; cave-home should mirror its behaviour line-by-line.
2. **The Hue Bridge as a target cave-home can *emulate*.** Some
   users (typically §2 persona 3–4) want to dispose of the
   physical Hue Bridge and have cave-home present itself as a
   Hue Bridge to third-party Hue apps / voice assistants /
   automation tools that still speak the Hue API. **diyhue** is
   the reference OSS project for this — but diyhue is GPL-3.0,
   so the clean-room mandate (Charter §6.1 / ADR-002) applies.

Native Hue Zigbee bulbs do not require any of this — they pair
through cave-home's Zigbee stack (`cave-home-zigbee`, ADR-001
scope, M2 ROADMAP). ADR-010 is about Bridge integration / bridge
emulation, not Zigbee bulb support.

## Decision

Two cave-home crates, distinct port methods:

1. **`cave-home-hue`** — Apache-2.0 **line-by-line** port of the
   HA `hue` integration. Talks to a physical Hue Bridge over
   the official Philips Hue API (v2 and v1 surfaces). This is
   the default Hue path for the §2 persona 1–2 user.
2. **`cave-home-hue-bridge-emu`** — **clean-room** Rust
   implementation of the Hue Bridge API surface from the
   Philips developer-portal documentation only (per ADR-002 /
   Charter §6.1). The clean-room mandate applies because
   diyhue itself is GPL-3.0; contributors to this crate **must
   not read diyhue source**. Hue API documentation is publicly
   available on the Philips developer portal, and that is the
   sole permitted reference.

   `cave-home-hue-bridge-emu` is **advanced-mode** — gated
   behind the Portal's Settings → "Developer view" toggle
   (ADR-007). The headline persona never sees the option to
   turn cave-home itself into a Hue Bridge.

## Consequences

### Accepted gains

- **Hue users covered without a clean-room mandate for the
  default path.** The 90% case (existing Hue Bridge + cave-home
  client) is a permissive Apache-2.0 port; only the bridge
  emulator carries the clean-room overhead.
- **Bridge disposal path** is available to §2 power-user
  personas who want to consolidate hardware on cave-home.
- **Apache-2.0 cleanliness** of `cave-home-hue` itself — the
  GPL diyhue derivation is fully isolated in
  `cave-home-hue-bridge-emu` and even there the crate is
  written from spec, not from diyhue source.

### Accepted costs

- **Two crates, two surfaces.** The bridge emulator has its
  own test fixtures (per Charter §6.1 — clean-room crates do
  not port the upstream's tests).
- **Hue API drift risk for the emulator.** Philips may add
  endpoints / change auth in the v2 API; the spec-based
  emulator must track those changes from public docs only.
- **Contributor recusal for `cave-home-hue-bridge-emu`.**
  Anyone who has read diyhue source previously is barred from
  contributing to this crate (Charter §6.1 / CONTRIBUTING.md).
  Reviewers do not grep diyhue either.

### Charter §6.3 / ADR-007 compliance

The UI never says "Hue Bridge API endpoint" or "diyhue-style
bridge emulator". Default users see "Hue lambası"; advanced
users (Developer view) see "cave-home'u Hue Bridge olarak
yayınla" toggle.

## Alternatives considered

### (a) Hue Bridge client only; drop the emulator

Skip `cave-home-hue-bridge-emu` entirely.

- **Rejected.** Bridge consolidation is a real §2 persona 3–5
  ask (one less always-on appliance, one less vendor account).
  The clean-room overhead is acceptable for an advanced-mode
  feature.

### (b) Use diyhue as a runtime dependency (sub-process)

Run diyhue as a sidecar.

- **Rejected.** Charter §5 single-binary mandate forbids
  sidecars. Even if we relaxed §5 here, vendoring a GPL daemon
  inside an Apache-2.0 cave-home distribution creates licence
  bleed problems we already declined to take on (ADR-002).

### (c) Native Zigbee only; no Bridge surface at all

Don't talk to Hue Bridges; tell users to pair Hue bulbs
directly into the cave-home Zigbee stack.

- **Rejected.** Native Zigbee works for the bulbs, but breaks
  user workflows that already depend on the Bridge (entertainment
  sync, schedules stored on the Bridge, Hue app shortcuts).
  Charter §2 persona 1–2 should not be forced to re-pair every
  bulb to migrate to cave-home.

## Open questions

1. **Hue Entertainment / sync.** The Bridge's Entertainment
   protocol (DTLS streaming) sits on top of the Bridge API.
   Initial port supports control + state; Entertainment is a
   stretch.
2. **Bridge-emulator scope ceiling.** The emulator targets the
   *control surface* of the v2 API; advanced features (rule
   engine, scenes-on-bridge) are a longer tail. Recorded for a
   future amending ADR.
