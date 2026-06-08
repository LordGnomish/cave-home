# Coverage matrix — cave-home-wellness

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · port method per manifest.
**Verified:** 21/21 mapped symbols found in source · 31 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| Validated metric value objects (steps, heart rate, sleep, active minutes, body weight) | src/metric.rs::{Steps,HeartRate,SleepDuration,ActiveMinutes,BodyWeight} | yes |
| Per-day metric aggregate keyed on caller-supplied day tick | src/metric.rs::DailyMetrics | yes |
| Goal engine: metric + target + period, clamped progress with met boundary | src/goal.rs::{Goal,progress,GoalProgress} | yes |
| Resting-heart-rate wellness band (Low/Normal/Elevated/High) | src/band.rs::RestingHrBand | yes |
| Sleep-duration wellness band (Insufficient/Adequate/Optimal/Excessive, ~7-9h guidance) | src/band.rs::SleepBand | yes |
| Step-activity wellness band (Sedentary/Low/Active/VeryActive) | src/band.rs::ActivityBand | yes |
| Grandma-friendly EN/DE/TR names + gentle non-clinical advice per band | src/band.rs::{RestingHrBand,SleepBand,ActivityBand}::{name,advice} | yes |
| Pure trend classifier (Improving/Steady/Declining) with relative dead-band | src/trend.rs::classify_trend | yes |
| Pure goal-streak counting (longest run + current run) | src/streak.rs::{longest_streak,current_streak} | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| Apple Health adapter (via HomeKit / HealthKit surface) | phase-1b | ADR-025: read-only surface via existing HomeKit bridge; OS/permission-bound; maps onto metric value objects then reuses engine |
| Google Fit adapter | phase-1b | ADR-025: cloud read API; account/OAuth-bound on user side; cave-home stays account-free; I/O adapter only |
| Fitbit adapter | phase-1b | ADR-025: line-by-line port of HA fitbit integration (Apache-2.0); cloud API, account-bound; I/O adapter only |
| Withings adapter (sleep / scale) | phase-1b | ADR-025: line-by-line port of HA withings integration (Apache-2.0); cloud or local hub; network/account-bound I/O adapter |
| Garmin Connect adapter | phase-1b | ADR-025: line-by-line port of HA garmin_connect integration (Apache-2.0); cloud API, account-bound; I/O adapter only |
| BLE heart-rate / smart-scale adapter | phase-1b | ADR-025: direct Bluetooth-LE profiles (0x180D heart-rate, weight-scale) for account-free local sensors; BLE/hardware-bound |
| Persistent on-device wellness history store | phase-1b | ADR-025 / Charter §9: multi-day series held on-device only; storage-bound; lands when on-device store stabilizes; engine already store-agnostic |
| cave-home-core entity/state + automation-trigger integration | phase-1b | ADR-025: surfaces wellness as core State entities + family-role-gated automation triggers; lands when core entity API stabilizes; engine already core-agnostic |
| Cloud upload / off-device sync of wellness data | permanent | Charter §9 + ADR-025: wellness data is most sensitive; computed and kept on-device only; cave-home never uploads to cloud; permanent boundary |

## Drift notes
None — every claimed symbol exists in source. All 21 mapped implementations verified present. Declared honest_ratio of 1.00 is fully supported: fill_ratio 0.30 is completely implemented and tested; zero unjustified gaps; all unmapped items carry ADR-025 or Charter disposition. Test coverage is spec-designed (not ported from upstream); 31 tests across metric validation, goal progress, band boundaries, UX copy integrity (jargon-free, non-clinical), trend classification, and streak counting.
