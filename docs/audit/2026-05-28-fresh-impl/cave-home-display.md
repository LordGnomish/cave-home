# Coverage matrix — cave-home-display

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · spec-based control engine (Phase 1 MVP).
**Verified:** 6/6 mapped symbols found in source · 44 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| media_player power model (on / off / standby) with active/visible queries | src/power.rs::PowerState | yes |
| MediaPlayerState playback set + play/pause/stop transition rules | src/playback.rs::PlaybackState | yes |
| volume_set/volume_mute: 0..=100 clamp, parental cap, mute-restore | src/volume.rs::Volume | yes |
| select_source / play_media: known-input + installed-app + app-capability gating | src/source.rs::{Source,App,SourceCatalog,AppCapability} | yes |
| media_player service command surface as power-gated state machine | src/machine.rs::{Display,MediaCommand,CommandError} | yes |
| Grandma-friendly EN/DE/TR status sentence (ADR-028) | src/label.rs::{Lang,Display::status_sentence} | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| LG webOS TV adapter (SSAP websocket) | phase-1b | ADR-028: Network-bound; maps webOS commands/state onto MediaCommand + Display. Line-by-line port of HA `webostv`. |
| Samsung Tizen TV adapter (websocket + WoWLAN wake) | phase-1b | ADR-028: Network-bound I/O adapter; routes onto engine. Line-by-line port of HA `samsungtv`. |
| Android TV / Google Cast adapter (ADB + Cast protocol) | phase-1b | ADR-028: Network-bound; both surface as MediaCommand/Display. Line-by-line port of HA `androidtv` + `cast`. |
| Apple TV adapter (MRP / Companion) | phase-1b | ADR-028: Network-bound I/O adapter with pairing; maps onto this engine. |
| Roku adapter (ECP HTTP) | phase-1b | ADR-028: Network-bound poller/sender; thin adapter onto MediaCommand. |
| HDMI-CEC bridge | phase-1b | ADR-028: Hardware-bound CEC adapter; maps PowerOn/PowerOff/SelectSource onto this engine. |
| DLNA / UPnP media-renderer adapter | phase-1b | ADR-028: Network-bound I/O adapter; routes onto playback verbs. |
| Now-playing metadata + artwork fetch | phase-1b | ADR-028: Vendor session APIs; metadata-agnostic control engine; additive read path layered once adapter connected. |
| cave-home-core entity/state integration | phase-1b | ADR-028: Core entity API stabilisation pending; engine is already core-agnostic. |
| Pre-existing vendor API version shims / legacy auth models | permanent | Charter §7 always-latest + §8 no-backcompat: targets current vendor APIs only. |

## Drift notes
None — every claimed symbol exists in source. Manifest declaration at fill=0.30 / honest=1.00 is fully supported by the implementation: control engine is complete; all deferred work carries explicit ADR-028 Phase 1b justification (9 items) or permanent Charter justification (1 item).
