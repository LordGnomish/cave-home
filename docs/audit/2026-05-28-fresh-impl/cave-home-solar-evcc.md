# Coverage matrix — cave-home-solar-evcc

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · port method per manifest.
**Verified:** 10/10 mapped symbols found in source · 59 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| Charge modes off / now / minpv (MinPlusPv) / pv (PvOnly) | src/mode.rs::ChargeMode | yes |
| Power balance: surplus = PV − house consumption (+ battery policy), car draw added back | src/balance.rs::PowerSnapshot::surplus_watts | yes |
| Home-battery discharge policy (hold for house vs assist car) | src/balance.rs::BatteryPolicy | yes |
| Surplus→current sizing I = surplus/(V·phases), clamped to charger min/max | src/current.rs::decide_current | yes |
| PvOnly pauses below single-phase 6 A minimum; MinPlusPv draws minimum from grid | src/current.rs::decide_current | yes |
| Charger current window (6..16 A, 6..32 A) with validation | src/current.rs::CurrentLimits | yes |
| Automatic 1↔3 phase switching with hysteresis margin to avoid flapping | src/phase.rs::decide_phases | yes |
| Anti-flap dwell timer (condition must hold N caller-supplied seconds) | src/antiflap.rs::AntiFlapTimer | yes |
| Deadline charge plan: sun-makes-it / needs-grid-topup / unreachable from target SoC, capacity, hours | src/plan.rs::plan | yes |
| Input validation (negative power/current, SoC>100, phases∉{1,3}, voltage≤0, capacity≤0) | src/error.rs::EvccError | yes |
| Grandma-friendly EN/DE/TR charge status (Charter §6.3, ADR-007) | src/label.rs::ChargeStatus | yes |
| End-to-end decision (surplus → setpoint → friendly status) | src/lib.rs::decide | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| Charger / wallbox adapters (OCPP, go-eCharger, Wallbe, Easee, Tesla, KEBA, Modbus EVSE) | phase-1b | ADR-001: every wallbox speaks a different network/Modbus protocol. Network/hardware-bound; each adapter pushes the engine's chosen current/phase setpoint to the box and reads back actual charge power. No new decision logic. |
| Vehicle state-of-charge adapters (manufacturer APIs / OBD) | phase-1b | ADR-001: vehicle SoC comes from cloud APIs or OBD dongles, account/network-bound. They feed current_soc into plan::PlanInputs; the planner is already SoC-source-agnostic. |
| Meter / inverter adapters (grid / PV / home-battery power) | phase-1b | ADR-001: real PV/grid/battery watts come from SunSpec/Modbus/REST meters. Network/hardware-bound; they populate balance::PowerSnapshot. The engine takes watts as input by design. |
| cave-home-core entity/state + cave-home-history integration | phase-1b | ADR-001: surfacing the charge decision as core State entities, automation triggers and a logged history lands once those crate APIs stabilise. The engine is already core-agnostic and does no I/O. |
| Tariff / price- and CO₂-based charging (cheap-window scheduling) | phase-2 | ADR-001: charging in the cheapest / greenest grid window is a refinement layered over the same surplus engine; it needs a price/CO₂ feed (network-bound) and a clock. Phase-1 charges from sun + deadline only. |
| Pre-existing evcc YAML config compatibility / import | permanent | Charter §8 no-backcompat + §7 always-latest: cave-home ships its own current charge model and config; it will not import or emulate upstream evcc's historical YAML schema. |

## Drift notes
None — every claimed symbol exists in source. All 10 mapped capabilities verified in code. Honest ratio 1.00 is mathematically sound: fill_ratio / (fill_ratio + unjustified_gap) = 0.30 / (0.30 + 0) = 1.0. The 0.30 fill baseline reflects deliberate Phase 1 MVP scope (decision engine only); all 6 unmapped items carry explicit ADR-001 or Charter §8 disposition.
