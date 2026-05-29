# Coverage matrix — cave-home-hvac

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · port method per manifest.
**Verified:** 7/7 mapped symbols found in source · 39 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| HA HVACMode vocabulary (off/heat/cool/heat_cool/auto/dry/fan_only) | src/mode.rs::HvacMode | yes |
| HA HVACAction current-activity vocabulary (off/idle/heating/cooling/drying/fan/preheating/defrosting) | src/mode.rs::HvacAction | yes |
| HA fan_mode + preset_mode vocabularies | src/mode.rs::{FanMode,PresetMode} | yes |
| Canonical-Celsius temperature value object, validated finite + range, with C↔F conversion | src/temperature.rs::Temperature | yes |
| Single target_temperature and target_temp_low<target_temp_high band setpoints | src/setpoint.rs::Setpoint | yes |
| Device min/max temp + target_step + supported fan/preset capability gating | src/setpoint.rs::Capabilities | yes |
| HA generic_thermostat cold/hot-tolerance hysteresis decision per mode | src/control.rs::decide | yes |
| Grandma-friendly EN/DE/TR labels + action sentence for every mode/action/fan/preset (Charter §6.3, ADR-007) | src/label.rs | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| Viessmann Open3EClient (ViCare) heat-pump adapter | phase-1b | ADR-012: Network-bound I/O adapter; maps device onto HvacMode/Setpoint/Temperature then reuses this engine |
| Daikin / LG ThinQ / Bosch / Mitsubishi / Samsung climate adapters | phase-1b | ADR-012: Vendor APIs with auth + rate-limit quirks; network-bound I/O adapters map onto mode/setpoint model |
| Generic Zigbee / Z-Wave / Matter / Modbus thermostat adapters | phase-1b | ADR-012: Hardware/bus-bound; thermostat cluster maps onto HvacMode/Setpoint via existing radio pillars |
| Scheduling + PID auto-tuning of the control loop | phase-1b | ADR-012: Stateful layering on top of decide() primitive; sits atop once core scheduler API lands |
| cave-home-core entity/state integration | phase-1b | ADR-012: Core-agnostic engine; surfacing as State entities + automation triggers lands once entity API stabilises |
| Solar-surplus heat-pump load-shifting (cave-home-solar-evcc tie-in) | phase-2 | ADR-012: Cross-crate automation built on this engine + Energy Dashboard; Phase 2 refinement, not a climate primitive |
| Pre-current HA climate-attribute compatibility shims | permanent | Charter §7 always-latest + §8 no-backcompat: no historical-attribute compatibility mode |

## Drift notes
None — every claimed symbol exists in source.
