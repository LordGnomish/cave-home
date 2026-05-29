# Coverage matrix — cave-home-vacuum

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · port method per manifest.
**Verified:** 9/9 mapped symbols found in source · 63 test fns · drift: no.

## MAPPED (implemented + claimed)

| Spec capability | Source symbol | Verified |
|---|---|---|
| HA vacuum states: idle/cleaning/returning/docked/paused/error (+ spot-cleaning, manual) | src/state.rs::VacuumState | yes |
| HA vacuum services: start/pause/stop/return_to_base/clean_spot/locate/set_fan_speed (+ clean_segments, clean_zones) | src/state.rs::VacuumCommand | yes |
| Command-application state machine with valid/invalid transition handling | src/machine.rs::Vacuum::apply | yes |
| Reaching-dock settles to Docked; error input gates further commands until cleared | src/machine.rs::Vacuum::{reached_dock,report_error,clear_error} | yes |
| Fan-speed presets (Off/Min/Low/Medium/High/Max/Turbo) with per-unit capability gating | src/fan.rs::{FanSpeed,FanCapability} | yes |
| Battery model (0..=100, charging vs discharging) + low-battery auto-return threshold | src/battery.rs::Battery | yes |
| Low-battery-while-cleaning -> Returning auto-return | src/machine.rs::Vacuum::update_battery | yes |
| Room/segment + zone value types and validated clean-segments request | src/map.rs::{Segment,Zone,VacuumMap::validate_segments} | yes |
| Fault taxonomy (brush/wheel/side-brush stuck, bin full, dustbin missing, lost, trapped, cliff, water-tank, generic) | src/error.rs::ErrorCode | yes |
| Grandma-friendly EN/DE/TR status label + fault explanation & advice (Charter §6.3, ADR-007) | src/label.rs | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)

| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| Valetudo local REST/MQTT control adapter | phase-1b | ADR-017: Network-bound transport layer; maps wire commands onto VacuumCommand engine and feeds state/battery/error events back. No new decision logic. |
| Vendor-cloud adapters (Roborock / Dreame / Viomi cloud) | phase-1b | ADR-017: For non-Valetudo users; cave-home prefers cloud-free Valetudo path. Optional I/O adapter only, reuses engine unchanged. |
| Live map / lidar rendering | phase-1b | ADR-017: Data/graphics surface over saved map and lidar scan. Segment/zone validation logic is already modelled; only rendering deferred. |
| cave-home-core entity/state integration | phase-1b | ADR-017: Integration with core State entity API for automations (clean kitchen after dinner; skip baby room during nap). Engine already core-agnostic; awaits core API stability. |
| Vacuum firmware re-flashing (Valetudo rooting) | permanent | ADR-017 explicit (Charter §8 no-backcompat): cave-home does NOT re-flash firmware. Rooting is a user-side operation, never in scope. |
| Matter Robot Vacuum cluster binding | phase-2 | ADR-017 alternative (b): Matter binding is a Phase 2 transport over the same engine; Valetudo path ships first in Phase 1b. |

## Drift notes

None — every claimed symbol exists in source. All 9 mapped items verified:
- VacuumState enum at src/state.rs:22
- VacuumCommand enum at src/state.rs:76
- Vacuum::apply method at src/machine.rs:170
- reached_dock/report_error/clear_error methods at src/machine.rs:301, 309, 319
- FanSpeed enum at src/fan.rs:14; FanCapability struct at src/fan.rs:48
- Battery struct at src/battery.rs:41
- Vacuum::update_battery method at src/machine.rs:332
- Segment struct at src/map.rs:17; Zone struct at src/map.rs:45; VacuumMap::validate_segments at src/map.rs:171
- ErrorCode enum at src/error.rs:14
- label.rs module provides EN/DE/TR state/fault labels per Charter §6.3

Test coverage includes all spec-derived scenarios: state transition validity, low-battery auto-return, capability gating, segment validation, error gating/clear, and plaintext UX (no implementation jargon). 63 tests distributed across 7 source files. Port method is spec-based (HA vacuum domain + Valetudo control surface), not vendor-ported firmware code.
