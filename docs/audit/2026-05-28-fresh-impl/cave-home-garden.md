# Coverage matrix — cave-home-garden

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · port method per manifest.
**Verified:** 9/9 mapped symbols found in source · 36 test fns · drift: no.

## MAPPED (implemented + claimed)

| Spec capability | Source symbol | Verified |
|---|---|---|
| Plant care-profile model (light need, soil-moisture band, frost sensitivity, temperature band) | src/plant.rs::{Plant,CareProfile} | yes |
| Common-plant care presets (frost-tenderness + light + moisture per kind) | src/plant.rs::PlantKind::profile | yes |
| Frost-risk classification (None/Watch/Warning/Danger) per frost-tenderness class | src/frost.rs::FrostRisk::classify | yes |
| Light-fit assessment (measured lux or coarse band vs. FullSun/PartShade/Shade need) | src/light.rs::{assess_lux,assess_band} | yes |
| Soil-moisture-band assessment (TooDry/Ideal/TooWet) vs. plant's ideal band | src/moisture.rs::assess | yes |
| Watering recommendation modelled for hand-off to cave-home-water (no runtime/valve here) | src/moisture.rs::recommend_water | yes |
| Ambient-temperature comfort assessment (TooCold/Comfortable/TooHot) | src/advice.rs::assess_temp | yes |
| Growing-season / dormancy flag from caller-supplied month (NH default + SH offset) | src/season.rs::{growing_season,growing_season_in} | yes |
| Combined care-advice engine (frost + light + soil + temp + water + season, one verdict) | src/advice.rs::advise | yes |
| Grandma-friendly EN/DE/TR care messages + single most-actionable headline (ADR-007, §6.3) | src/advice.rs::CareAdvice::headline | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)

| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| Outdoor soil-moisture / light / temperature sensor adapters (Zigbee / BLE / ESPHome) | phase-1b | ADR-029: hardware/BLE-bound; adapters map readings onto engine inputs unchanged |
| Live weather-forecast feed (Davis Vantage, Ecowitt, Netatmo Weather) | phase-1b | ADR-029: network-bound; feeds forecast_min_c into FrostRisk::classify |
| Plant-database import (species → CareProfile) | phase-1b | ADR-029: data/IO concern; CareProfile model already accepts arbitrary profiles |
| cave-home-water integration (carry out the watering recommendation) | phase-1b | ADR-029 + ADR-013: boundary modelled as WaterRecommendation (flag + reason); cross-crate glue deferred |
| cave-home-core entity/state integration + automation triggers | phase-1b | ADR-029: deferred until cave-home-core entity API stabilizes; engine already core-agnostic |
| Husqvarna Automower / Worx Landroid robot-mower control | phase-1b | ADR-029: line-by-line port of HA Apache-2.0 integrations; cloud/network-bound and orthogonal to plant-care engine |
| Sub-daily / multi-day weather extrapolation for frost timing | phase-2 | ADR-029: single forecast minimum is correct for tonight's verdict; multi-day smoothing deferred to Phase 2 |
| Historical-snapshot / legacy threshold compatibility mode | permanent | Charter §7 (always-latest) + §8 (no-backcompat): ships current thresholds only; legacy mode rejected |

## Drift notes

None — every claimed symbol exists in source. All 9 mapped symbols verified in crates/cave-home-garden/src/. All 6 unmapped items carry explicit ADR-029 Phase 1b or later disposition. Declared honest_ratio (1.00 = 0.30 / 0.30) is correctly supported: no unjustified gap exists beyond the 0.30 fill_ratio.
