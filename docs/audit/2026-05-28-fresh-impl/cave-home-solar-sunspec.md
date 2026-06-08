# Coverage matrix — cave-home-solar-sunspec

**Declared:** fill=0.28 · adr_justified=1.00 · honest=1.00 · port method: spec-based (public SunSpec Alliance information-model spec, no GPL pysunspec/library porting).
**Verified:** 9/9 mapped symbols found in source · 50 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| SunSpec point types int16/uint16/int32/uint32 with not-implemented sentinels | src/point.rs::{int16,uint16,int32,uint32} | yes |
| SunSpec acc32 accumulator and float32 point types | src/point.rs::{acc32,float32} | yes |
| SunSpec string point (register-pair MSB-first ASCII, NUL/space padded) | src/point.rs::string | yes |
| SunSpec sunssf scale factor: real value = raw * 10^sf | src/scale.rs::ScaleFactor | yes |
| SunSpec model discovery: 'SunS' marker check + chain walk to 0xFFFF | src/discovery.rs::{check_marker,walk_chain,discover} | yes |
| SunSpec Common model 1 (manufacturer / model / version / serial / device address) | src/common.rs::CommonModel | yes |
| SunSpec inverter models 101/102/103 (integer + scale factor) decode | src/inverter.rs::InverterReading::decode_integer | yes |
| SunSpec inverter models 111/112/113 (float32) decode | src/inverter.rs::InverterReading::decode_float | yes |
| SunSpec inverter St operating-state enum (off/sleeping/starting/MPPT/throttled/shutting-down/fault/standby) | src/inverter.rs::OperatingState | yes |
| Bounds-checked register slicing (OutOfBounds + LengthMismatch errors) | src/point.rs + src/discovery.rs | yes |
| Grandma-friendly EN/DE/TR solar status + 'producing N.N kW' phrasing | src/label.rs::{SolarStatus,describe} | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| Live Modbus-TCP transport (read holding registers over the network) | phase-1b | ADR-019: decoder is transport-agnostic (&[u16] operation). TCP reader outside scope; pure decode engine complete. |
| Modbus-RTU / serial transport | phase-1b | Serial-port-bound I/O sibling of TCP; hardware-bound, deferred with transport layer. |
| Device polling loop + caching | phase-1b | Periodic poller + debouncing; timer/runtime-bound, lands with transport. Decode core complete. |
| SunSpec meter models (201-204) | phase-1b | Grid/consumption meters; reuse point/scale primitives already in src/point.rs + src/scale.rs. Additive decode; deferred until transport surfaces a meter. |
| SunSpec storage / battery model (124) | phase-1b | Battery state-of-charge / charge-discharge; reuses implemented point/scale primitives. Additive; deferred with storage-inverter transport path. |
| SunSpec multiple-MPPT model (160) | phase-1b | Per-string MPPT extension; repeating-block decode over same primitives, additive, deferred to Phase 1b. |
| cave-home-core entity/state + cave-home-history integration | phase-1b | ADR-019: surfacing readings as core State entities + writing to history lands once those crates' APIs stabilise. Decoder already core-agnostic. |
| Vendor-private models (64xxx) decode | phase-2 | SMA / Fronius / SolarEdge vendor extensions are non-standard, per-vendor. Non-standard; deferred until concrete household need justifies per-vendor reverse-engineering. Standard models cover all six families' AC/DC power, energy, frequency, state. |
| Pre-revision SunSpec model compatibility shims | permanent | Charter §7 always-latest + §8 no-backcompat: cave-home decodes current public SunSpec model definitions only; no historical-snapshot or deprecated-layout mode. |

## Drift notes
None — every claimed symbol exists in source. All 9 mapped symbols verified via grep in src/point.rs, src/scale.rs, src/discovery.rs, src/common.rs, src/inverter.rs, src/label.rs. No symbols claimed but absent. Declared honest_ratio (1.00) is supported: every unfilled item carries explicit phase disposition (phase-1b, phase-2, permanent).
