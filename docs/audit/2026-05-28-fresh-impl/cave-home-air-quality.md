# Coverage matrix — cave-home-air-quality

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · port method per manifest.
**Verified:** 8/8 mapped symbols found in source · 29 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| EPA AQI piecewise-linear formula I_p=(I_hi-I_lo)/(C_hi-C_lo)*(C_p-C_lo)+I_lo | src/aqi.rs::sub_index | yes |
| EPA 2024 PM2.5 / PM10 / O3 / NO2 / SO2 / CO breakpoint tables | src/aqi.rs::{PM25,PM10,OZONE,NO2,SO2,CO} | yes |
| EPA concentration truncation-to-table-precision rule | src/aqi.rs::truncate | yes |
| CO₂ indoor-air classification (OSHA-anchored) | src/classify.rs::classify_co2 | yes |
| Sensirion VOC Index classification (1..=500) | src/classify.rs::classify_voc | yes |
| Radon classification (WHO 100 / EPA 148 Bq/m³) | src/classify.rs::classify_radon | yes |
| Six-band grandma-friendly category + EN/DE/TR name & advice (ADR-007) | src/category.rs::AirCategory | yes |
| Worst-of room aggregation with dominant-pollutant attribution | src/assessment.rs::assess | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| AirGradient local HTTP API adapter | phase-1b | ADR-019: Network-bound HTTP poller; routes /measures JSON to reading::Reading, reuses engine core |
| Awair local + cloud API adapter | phase-1b | ADR-019: Account-bound cloud avoided; cave-home stays account-free via local API preference |
| IKEA Vindriktning PM2.5 adapter | phase-1b | ADR-019: Hardware-bound UART/ESPHome or Zigbee; routes single PM2.5 reading into engine |
| Airthings radon adapter (BLE + cloud) | phase-1b | ADR-019: BLE/network-bound I/O; only sovereign radon path for that sensor class |
| cave-home-core entity/state integration | phase-1b | ADR-019: Deferred until cave-home-core entity API stabilizes; engine is already core-agnostic |
| NowCast time-weighted averaging | phase-2 | ADR-019: Instantaneous AQI correct for live tile; EPA NowCast is Phase 2 24-hour refinement |
| Pre-2024 PM2.5 breakpoint compatibility | permanent | Charter §7 always-latest + §8 no-backcompat: ships current EPA breakpoints only |

## Drift notes
None — every claimed symbol exists in source. All mapped implementations verified in aqi.rs, classify.rs, category.rs, assessment.rs, reading.rs. All unmapped items carry explicit ADR-019 disposition anchoring fill_ratio=0.30 to adr_justified_ratio=1.00, supporting honest_ratio=1.00.
