# Coverage matrix — cave-home-solar-forecast

**Declared:** fill=0.45 · adr_justified=1.00 · honest=1.00 · spec-based port (public-domain astronomy + clear-sky model).
**Verified:** 22/22 mapped symbols found in source · 61 test fns · drift: no.

## MAPPED (implemented + claimed)

| Spec capability | Source symbol | Verified |
|---|---|---|
| Spencer (1971) solar declination Fourier series | src/sun_position.rs::declination_deg | yes |
| Spencer (1971) equation of time | src/sun_position.rs::equation_of_time_min | yes |
| Hour angle & UTC-to-true-solar-time conversion | src/sun_position.rs::{hour_angle_deg,true_solar_time_h} | yes |
| Solar elevation & azimuth geometry | src/sun_position.rs::position_from_angles | yes |
| Sunrise hour angle & daylight length | src/sun_position.rs::{sunrise_hour_angle_deg,daylight_hours} | yes |
| Earth-Sun-distance eccentricity correction | src/irradiance.rs::extraterrestrial_normal_w_m2 | yes |
| Kasten & Young (1989) air mass | src/irradiance.rs::air_mass | yes |
| Clear-sky beam & global-horizontal irradiance | src/irradiance.rs::{clear_sky_dni_w_m2,clear_sky_ghi_w_m2} | yes |
| Kasten-Czeplak cloud-cover derate | src/irradiance.rs::cloud_derate | yes |
| Plane-of-array angle-of-incidence | src/array.rs::cos_incidence | yes |
| Plane-of-array irradiance & AC power | src/array.rs::{plane_of_array_w_m2,ac_power_kw} | yes |
| PV array validation (peak/tilt/azimuth/derate) | src/array.rs::PvArray::new | yes |
| Daily-energy integral & peak detection | src/forecast.rs::forecast_day | yes |
| Site & forecast input validation | src/forecast.rs::{Site::new,instant_at} | yes |
| Three-band grandma-friendly UX (EN/DE/TR) | src/label.rs::{SolarOutlook,peak_time_phrase,daily_summary} | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)

| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| Forecast.Solar live forecast API adapter | phase-1b | Network-bound; supplies clearness fraction into forecast_day; pure I/O adapter, reuses core engine unchanged |
| Solcast live forecast API adapter | phase-1b | Account-bound rooftop-PV forecast; cloud-cover mapping adapter only; network-bound I/O |
| Open-Meteo / DWD weather-feed adapter | phase-1b | Free cloud-cover forecasts; network-bound; maps cloud fraction to engine input directly |
| Horizon / shading profile model | phase-1b | Terrain/building masks clip clear-sky beam at low sun; geometry layer atop sun-position; needs per-site survey or DEM |
| cave-home-core entity/state + cave-home-history integration | phase-1b | Forecasts as State entities, automation triggers, actuals persistence; deferred pending API stabilization |
| Machine-learning production correction from historical actuals | phase-2 | Learned bias & soiling correction; depends on cave-home-history pillar and metered actuals window |
| Pre-computed irradiance lookup tables / frozen ephemeris snapshot | permanent | Charter 7 always-latest + §8 no-backcompat: computed at call time, no frozen tables shipped |

## Drift notes

None — every claimed symbol exists in source. All 22 mapped capabilities verified in /crates/cave-home-solar-forecast/src. Test coverage across five modules: sun_position (15), irradiance (12), array (14), forecast (15), label (5). ADR-002 and ADR-023 justify all phase-1b deferrals and scope cuts; honest_ratio = 1.00 holds (fill=0.45, unjustified_gap=0).
