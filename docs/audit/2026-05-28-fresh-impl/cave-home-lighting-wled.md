# Coverage matrix — cave-home-lighting-wled

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · clean-room port method per manifest.
**Verified:** 10/10 mapped symbols found in source · 50 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| Colour byte model: RGB / RGBW slots | src/color.rs::{Rgb,Rgbw} | yes |
| HSV↔RGB conversion + Kelvin→RGB approximation | src/color.rs::{Hsv,kelvin_to_rgb} | yes |
| WLED segment model (id/start/stop/col/fx/pal/sx/ix/bri/on/rev/mir) | src/segment.rs::Segment | yes |
| WLED state object (on/bri/transition/ps/seg/nl) | src/state.rs::State | yes |
| WLED nightlight block (on/dur/tbri/mode) | src/state.rs::Nightlight | yes |
| State JSON round-trip (encode/decode/partial-parse/byte-clamping) | src/state.rs::{to_json,from_json} | yes |
| Minimal std-only JSON value model + parser/serializer | src/json.rs::Json | yes |
| Built-in effect & palette registries with validation bounds | src/effect.rs::{EFFECTS,PALETTES,effect,palette,effect_name,MAX_EFFECT_ID,MAX_PALETTE_ID} | yes |
| Pure validated control layer (brightness/toggle/power/colour/effect/palette/preset/nightlight) | src/command.rs::{Command,CommandError} | yes |
| Grandma-friendly EN/DE/TR wording (colour word, brightness %, headline) | src/{label.rs,command.rs::headline} | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| HTTP/JSON request transport to real WLED device | phase-1b | ADR-014 / ROADMAP M8: control brain produces/consumes state JSON; HTTP poller is pure network I/O on top. |
| WebSocket live-state channel (ws://<device>/ws) | phase-1b | ADR-014 / ROADMAP M8: documented channel pushes state deltas through same State::from_json path. |
| UDP realtime protocols (DDP / E1.31 / DRGB / WARLS) for per-LED streaming | phase-1b | ADR-014 / ROADMAP M8: network/timing-bound; only needed for music-reactive/video use (ADR-020 audio pillar). |
| mDNS device discovery (_wled._tcp) | phase-1b | ADR-014 / ROADMAP M8: shared network-service-discovery concern; control brain addressed by IP/host, needs no discovery. |
| Full 100+ built-in effect & palette enumeration | phase-1b | ADR-014: curated subset (26 effects + 14 palettes) ships with names; device reports full list over JSON API (data-population task). |
| RGBW / CCT white-channel auto-calculation from RGB | phase-1b | ADR-014: Rgbw type + Kelvin→RGB exist; channel derivation refinement tied to live device capabilities report. |
| cave-home-core entity/state integration + automation triggers | phase-1b | ADR-014 / ROADMAP M8: surfaces light as core State entity once cave-home-core entity API stabilises. |
| WLED preset & playlist *definitions* (storing/editing preset slots) | phase-2 | ADR-014: Phase 1 can *apply* preset by id; authoring/persisting definitions is Phase 2 management feature. |
| Pre-0.14 / legacy WLED JSON API field compatibility | permanent | Charter §8 no-backcompat + §7 always-latest: targets current JSON API only; no historical mode. |
| WLED firmware source reuse | permanent | Charter §6.1 / ADR-014: WLED treated as copyleft; may not read or port source. Clean-room only from public JSON API docs. |
| 32-bit ARM / pre-Linux 7.1 kernels | permanent | Charter §6.2 / ADR-003: Linux 7.1+ only. |

## Drift notes
None — every claimed symbol exists in source.
