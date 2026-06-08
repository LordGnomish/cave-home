# Coverage matrix — cave-home-cover

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · port method per manifest.
**Verified:** 11/12 mapped symbols found in source · 35 test fns · drift: YES.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| Cover position validated 0..=100 percentage | src/position.rs::Position | yes |
| Cover motion states (open/closed/opening/closing/stopped) | src/state.rs::CoverState | yes |
| Cover device classes (9 types) | src/device_class.rs::DeviceClass | yes |
| Supported-features model (set_position/tilt/stop) | src/device_class.rs::Features | yes |
| Cover commands (open/close/stop/set_position/tilt) | src/machine.rs::CoverCommand | yes |
| Position state machine: apply command, settle at-rest | src/machine.rs::Cover::apply | yes |
| Travel-direction inference (opening/closing/no-move) | src/machine.rs::Cover::begin | yes |
| Travel-direction inference (opening/closing/no-move) | src/machine.rs::Cover::direction | **NO** |
| Reject unsupported commands (set_position/tilt) | src/machine.rs::CommandError | yes |
| Stop always honoured even without stop feature | src/machine.rs::Cover::apply (CoverCommand::Stop branch) | yes |
| Obstruction safety override forces Stopped | src/machine.rs::Cover::report_obstruction | yes |
| Independent tilt axis (0..=100) | src/machine.rs::Cover::apply (CoverCommand::SetTiltPosition branch) | yes |
| Grandma-friendly localised status (EN/DE/TR) | src/label.rs::status_sentence | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| OpenGarage HTTP controller adapter | phase-1b | ADR-015: Network-bound I/O adapter; maps onto engine then reuses unchanged. Line-by-line port (Apache-2.0). |
| ESPHome cover bindings adapter | phase-1b | ADR-015: ESPHome components reach cave-home via existing adapter; this glue routes position/tilt/state onto engine. Line-by-line (MIT). I/O adapter only. |
| Somfy RTS RF adapter (clean-room) | phase-1b | ADR-015: One-way RF over RFXtrx-class transceiver dongle (hardware-floor add). Clean-room from public RF spec; no third-party reverse-engineering read. |
| Zigbee / Z-Wave / Matter / MQTT transports | phase-1b | ADR-015: Covers reach via existing protocol pillars; glue maps their position/tilt onto engine. This crate handles only upstreams not covered by pillars. |
| Real motor travel-time calibration & position estimation | phase-1b | ADR-015: Position-only covers estimate openness from calibrated full-travel time. Engine models begin/settle_at; timing/calibration is hardware-bound with adapters. |
| cave-home-core entity/state integration | phase-1b | ADR-015: Surfacing covers as core State entities + automation triggers lands when cave-home-core entity API stabilises. Engine already core-agnostic. |

## Drift notes
**`src/machine.rs::Cover::direction` is a PRIVATE function (line 223: `fn direction`) but is claimed as a mapped public symbol.** The manifest lists it in entry [[mapped]] line 75 as part of `Cover::{begin,direction}` for "Travel-direction inference". Only `begin` is public; `direction` is internal utility used by `begin`. The declared honest_ratio=1.00 is therefore unsupported by code — there is 1 symbol claimed but not actually exported as public API.
