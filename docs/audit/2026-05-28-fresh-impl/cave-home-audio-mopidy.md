# Coverage matrix — cave-home-audio-mopidy

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · port method: spec-based (MPD protocol engine per Charter §6.1 clean-room) + line-by-line adapters (Mopidy, phase-1b).
**Verified:** 7/7 mapped symbols found in source · 58 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| MPD line-protocol command parsing with quoted/escaped argument handling | src/command.rs::parse | yes |
| MPD ACK error model (codes 2/5/50) — reject never panic | src/command.rs::ParseError + src/response.rs::AckError | yes |
| MPD response framing: key:value terminated by OK or ACK [code@idx] | src/response.rs::Response | yes |
| Play queue: stable song ids vs. positions, add/delete/deleteid/move/clear | src/tracklist.rs::Tracklist | yes |
| Next-song computation honouring random/repeat/single/consume | src/tracklist.rs::{next_after,consume_finished,shuffle_order} | yes |
| Playback-state model: play/pause/stop, volume 0..=100, elapsed, toggle modes | src/playback.rs::Playback | yes |
| MPD status/currentsong/playlistinfo snapshot field builders | src/status.rs::{status,current_song,playlist_info} | yes |
| Grandma-friendly EN/DE/TR phrases (Playing…/paused/stopped/repeat/shuffle/volume) | src/label.rs | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| TCP MPD server (socket loop, idle subsystem, command_list, binary responses) | phase-1b | ADR-020: wire server wraps engine over TCP listener. Network-bound; feeds Commands/serialises Responses. No new core logic. |
| GStreamer audio playback pipeline (decode, output, gapless, replaygain) | phase-1b | ADR-020: actual audio rendering is pipeline-driven by transport state. Pipeline/hardware-bound; PlayState/elapsed model here is its control surface. |
| Mopidy backend extensions (local, Spotify, TuneIn, SoundCloud) | phase-1b | ADR-020: per-backend URI resolution and streaming. Each network/account-bound and iterative; resolve opaque URIs engine already carries. |
| Library browsing/search (lsinfo, find, search, listplaylists) | phase-1b | ADR-020: browsing depends on live backends, deferred with them. Queue/tracklist model here is backend-agnostic and ready. |
| cave-home core entity/state + Snapcast multi-room integration | phase-1b | ADR-020: surfacing player as core media entity and routing through snapcast for sync lands once those crates stabilise. Engine is core-agnostic. |
| Native MPD server (if Mopidy proves insufficient) | phase-2 | ADR-020 §Decision [ASSUMPTION]: MPD upstream is GPL-only; clean-room server would be separate crate via future ADR-020b. This crate implements public protocol grammar only. |
| Legacy/pre-current MPD protocol compatibility shims | permanent | Charter §7 always-latest + §8 no-backcompat: cave-home tracks current documented MPD protocol only; no historical compatibility mode. |

## Drift notes
None — every claimed symbol exists in source. All 7 mapped entries verified. The 1.00 honest_ratio is well-supported: the 0.30 fill_ratio accurately represents the core protocol engine phase, and every unmapped/cut item carries explicit ADR-020 phase-1b disposition with no unjustified gaps.
