# ADR-014 — Lighting (WLED + LED strips) — clean-room mandate

## Status

**Accepted** — 2026-05-15, founder wholesale approval (Charter v6
expansion).

## Context

WLED is the dominant OSS firmware for ESP-based addressable LED
strips (WS2812, SK6812, etc.) — the Charter §2 family persona's
"under-cabinet lighting", "TV bias lighting", "kid's bedroom RGB"
use case. WLED's GitHub repo carries a non-permissive licence
(EUPL-1.2 / GPL-aligned for the firmware portion) [ASSUMPTION
based on founder dispatch flagging WLED as clean-room].

Diyhue (the Hue Bridge emulator) is already covered by
`cave-home-hue-bridge-emu` (ADR-010) under the same clean-room
mandate; ADR-014 does not duplicate it.

Music-reactive lighting and scene effects integrate via the
existing automation engine + audio pillar (ADR-020).

## Decision

`cave-home-lighting-wled` — **clean-room** Rust reimplementation
of the WLED JSON API and ESP-side wire format, from the public
WLED protocol documentation only. Contributors must **NOT** read
WLED source.

Port method: **clean-room** per Charter §6.1 / ADR-002.
Spec sources:

- WLED JSON API public docs (kno.wled.ge)
- WLED UDP realtime protocol public spec
- WS2812 / SK6812 / NeoPixel public timing diagrams

## Consequences

### Accepted gains
- WLED-flashed strips reach cave-home without forcing the user
  through HA's `wled` add-on.
- Music-reactive effects tie cleanly into the audio pillar.

### Accepted costs
- Clean-room contributor recusal: anyone who has read WLED
  source is barred from this crate (CONTRIBUTING.md).
- Effects-engine richness is whatever the public protocol
  exposes — bespoke effects that only run on WLED firmware
  are not portable.

### Charter §6.3 / ADR-007 compliance
UI says "Mutfak şerit aydınlatma", "TV arkası renkli",
"Müzik moduna geç" — never "WLED JSON API", "UDP realtime
protocol", "preset slot".

## Alternatives considered

- (a) Skip WLED, support only Zigbee/Matter lighting. Rejected —
  WLED is too ubiquitous in the maker/family-lighting overlap.
- (b) Vendor WLED firmware as a sidecar. Rejected — Charter §5
  single-binary mandate + GPL-bleed concerns.

## Notes

[ASSUMPTION: WLED is treated as copyleft for cave-home licensing
purposes. If subsequent audit shows it is actually permissive,
this ADR can be amended to allow line-by-line port — but
contributors continue under the clean-room rule until the
amendment lands. Conservative default protects Apache-2.0
cleanliness.]
