# Coverage matrix — cave-home-water

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · port method per manifest.
**Verified:** 8/8 mapped symbols found in source · 36 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| Vendor-neutral watering-circuit model with validated runtime/threshold/flow | src/zone.rs::Zone | yes |
| Zone lifecycle states (Idle/Watering/Paused/RainDelayed/Disabled) | src/zone.rs::ZoneState | yes |
| Watering decision engine — water now & for how long, with priority-ordered skips | src/decision.rs::decide | yes |
| Seasonal/weather % runtime adjustment (0%..=200%+, round-to-nearest, saturating) | src/decision.rs::apply_seasonal_adjust | yes |
| Explainable skip reasons (soil moist / rain delay / disabled / window / seasonal-zero) | src/decision.rs::SkipReason | yes |
| Skip reasons with explanation | src/decision.rs::WaterDecision | yes |
| Flow-fault detection: no-flow (stuck valve / cut supply) vs over-flow (burst pipe / leak) on a tolerance band | src/flow.rs::detect | yes |
| Flow fault types | src/flow.rs::FlowFault | yes |
| Sequential (non-concurrent) multi-zone run plan + total cycle duration | src/schedule.rs::plan_run | yes |
| Run plan with steps and total duration | src/schedule.rs::RunPlan | yes |
| Grandma-friendly EN/DE/TR labels & advice (Charter §6.3, ADR-007) | src/label.rs::Lang | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| OpenSprinkler API-client adapter | phase-1b | ADR-013: port the OpenSprinkler open-API client (old + new generation surfaces) — NOT the GPLv3 firmware. Network-bound; maps station runtimes + rain-delay onto zone::Zone then reuses this engine. |
| Rachio / B-hyve / cloud-irrigation adapters | phase-1b | ADR-013: cloud-account-bound irrigation controllers. cave-home prefers local control; these route schedule + flow telemetry into the engine. I/O adapter only, no new decision logic. |
| Zigbee / Z-Wave valve adapters | phase-1b | ADR-013: drip/hose valves driven over cave-home-zigbee / Z-Wave. Hardware-bound; surfaces a ZoneState and a measured flow reading into the engine. |
| Smart water-meter + leak-sensor I/O (S0 pulse / Modbus-RTU / M-Bus, Zigbee leak sensors) | phase-1b | ADR-013: water-meter protocols reach cave-home via the HA mbus/modbus integrations; Zigbee leak sensors via cave-home-zigbee. Bus/network-bound; the flow-fault model already consumes the readings these produce. |
| Live weather / ET0 evapotranspiration feed for the seasonal adjustment | phase-1b | ADR-013: the engine takes seasonal_percent + rain_delayed as inputs; deriving them from a live forecast / ET0 model is a network-bound feed that feeds this engine unchanged. The math here is correct for any supplied percentage. |
| cave-home-core entity/state integration | phase-1b | ADR-013: surfacing zones, decisions and leak alerts as core State entities + automation triggers (notification, valve shutoff) lands once cave-home-core's entity API stabilises. The engine is already core-agnostic. |
| Real timezone-aware scheduling triggers | phase-1b | ADR-013: deciding *when* the watering window opens is clock/timezone-bound. The engine takes within_window as an input (caller supplies the clock) so it stays a pure function; the trigger scheduler is the deferred I/O layer. |
| Concurrent multi-zone watering (pressure-managed parallel groups) | phase-2 | ADR-013: Phase 1 runs zones strictly sequentially to preserve supply pressure (OpenSprinkler default). Pressure-aware parallel groups are a Phase 2 refinement over the same run-plan model. |
| Legacy controller backward-compatibility mode | permanent | Charter §7 always-latest + §8 no-backcompat: cave-home ships the current decision model only; no historical controller-behaviour snapshot mode. |

## Drift notes
None — every claimed symbol exists in source.
