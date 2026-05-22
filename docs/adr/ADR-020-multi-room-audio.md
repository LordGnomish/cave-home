# ADR-020 — Multi-room audio (Music Assistant + Snapcast + Mopidy)

## Status

**Accepted** — 2026-05-15, founder wholesale approval (Charter v6
expansion).

## Context

Multi-room synchronised audio (kitchen + living room playing the
same playlist with no echo) is a Charter §2 persona-1 / 2 use
case that HA core orchestrates via **Music Assistant**.
Synchronisation underneath is provided by **Snapcast**; the local
music server is **Mopidy** (or **MPD**, its predecessor). Each
upstream has a different licence and port-method posture.

## Decision

Three crates, each port-method-appropriate:

`cave-home-audio-mass` — line-by-line port of Music Assistant
(Apache-2.0) HA-integrated orchestration.

`cave-home-audio-snapcast` — **clean-room** Rust reimplementation
of the Snapcast wire protocol from the public protocol
documentation. Snapcast upstream is GPL-3.0; contributors must
NOT read Snapcast source.

`cave-home-audio-mopidy` — line-by-line port of Mopidy
(Apache-2.0) backend client + HA Mopidy integration. **MPD
support is deferred** [ASSUMPTION: MPD's GPL-only status means
a future ADR-020b might add `cave-home-audio-mpd` clean-room if
Mopidy proves insufficient; not in scope for this commit].

Port methods:
- `cave-home-audio-mass`: line-by-line (Apache-2.0)
- `cave-home-audio-snapcast`: **clean-room** (Charter §6.1)
- `cave-home-audio-mopidy`: line-by-line (Apache-2.0)

## Consequences

### Accepted gains
- Synchronised house-wide audio without a vendor-cloud
  account (Sonos, Apple Music, Spotify Connect remain
  optional sources).
- TTS / voice-assistant playback (whisper.cpp + piper) routes
  through the same multi-room sync infrastructure.

### Accepted costs
- Snapcast clean-room is the largest clean-room sub-port in
  this wave; protocol is moderately complex (TCP control +
  PCM stream + time-sync).
- Mopidy plugin ecosystem (Spotify, TuneIn, etc.) is wide;
  per-plugin support is iterative.

### Charter §6.3 / ADR-007 compliance
UI says "Mutfakta + Salonda aynı şarkı", "Akşam yemeği müziği",
"Sessize al" — never "Snapcast TCP port", "Music Assistant
player UUID".

## Alternatives considered

- (a) Skip Snapcast; use Music Assistant's own
  synchronisation. Rejected — MASS's sync depends on per-
  player firmware; Snapcast is the universal sync layer.
- (b) Defer Mopidy; require users to bring an external music
  server. Rejected — cave-home's "one binary" promise
  includes local playback.

## Notes

[ASSUMPTION: Snapcast wire-protocol documentation is in the
public GitHub repository's docs/ directory but tagged in a way
that allows reading the *documentation* without reading the
*source*. Contributors are instructed to read only the
documentation, never the source files. Reviewer practice
mirrors the Charter §6.1 protocol established for Zigbee2MQTT
and AdGuard.]
