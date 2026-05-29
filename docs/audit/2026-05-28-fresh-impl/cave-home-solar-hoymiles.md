# Coverage matrix — cave-home-solar-hoymiles

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · clean-room port from public protocol description.
**Verified:** 11/11 mapped symbols found in source · 44 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| CRC-8/NRSC-5 (poly 0x31, init 0xFF) per-fragment checksum | src/crc.rs::crc8 | yes |
| CRC-16/Modbus (poly 0x8005 reflected, init 0xFFFF) assembled-payload checksum | src/crc.rs::crc16_modbus | yes |
| Inverter serial number + radio address derivation | src/frame.rs::InverterSerial | yes |
| Request command opcodes (RealTimeData / DeviceInfo / AlarmData) | src/frame.rs::Command | yes |
| Numbered multi-fragment response reassembly (in-order / out-of-order / missing / duplicate detection) | src/reassembly.rs::Reassembler | yes |
| Per-fragment CRC-8 validation on receive | src/reassembly.rs::Fragment::checksum_ok | yes |
| Real-time data record decode — per-DC-channel PV V/I/P, grid V/Hz, AC power, yield with fixed-point scaling | src/telemetry.rs::decode | yes |
| Assembled-payload CRC-16/Modbus verification + truncation rejection | src/telemetry.rs::decode | yes |
| Inverter family model — HM one/two/four-panel channel counts | src/family.rs::Family | yes |
| Operating/alarm-state mapping to grandma-friendly status (Charter §6.3, ADR-007) | src/label.rs::SolarStatus | yes |
| Localised EN/DE/TR status headline + advice + implementation-agnostic UX | src/label.rs::headline | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| NRF24L01 radio driver (SPI channel hopping, pipe addressing, ACK) | phase-1b | ROADMAP M5: hardware-bound I/O layer on top of decode brain; clean-room from datasheet |
| CMT2300A radio driver (SPI) for HMS/HMT newer units | phase-1b | ROADMAP M5: sub-GHz variant with same fragment/decode model, different SPI front-end; hardware-bound |
| DTU poll loop (request scheduling, response timeout/retry) | phase-1b | ROADMAP M5: async loop driving Reassembler on top of frame.rs + reassembly.rs |
| Power-limit command transmission (active-power throttle write path) | phase-1b | ROADMAP M5: set-power-limit TX opcode/frame; read-focused decode in Phase 1 |
| Device-info + alarm-data payload decoders | phase-1b | ROADMAP M5: Command opcodes modelled; only real-time-data decoded in Phase 1; full alarm-log Phase 1b |
| cave-home-core entity/state integration | phase-1b | ROADMAP M5: Telemetry as core State entities + automation triggers await core entity API stabilisation |
| cave-home-history time-series persistence (yield/power charts) | phase-1b | ADR-023: downstream consumer of Telemetry; no new decode logic required |
| Pre-Hoymiles-3.0 / legacy inverter framing | permanent | Charter §8 no-backcompat + §7 always-latest: HM/HMS/HMT only; no historical-protocol mode |
| 32-bit ARM / pre-Linux 7.1 kernels | permanent | Charter §6.2 / ADR-003: Linux 7.1+ target only |
| AhoyDTU / OpenDTU GPL-3.0 source reuse | permanent | Charter §6.1 / ADR-002: upstream GPL may not be read or ported; clean-room mandate |

## Drift notes
None — every claimed symbol exists in source. All 11 mapped entries (spec_section + spec_test) successfully located. No missing or misnamed symbols. Honest ratio 1.00 is supported: fill_ratio 0.30 over comprehensive scope (decode brain fully realized; hardware/radio layers correctly deferred to phase-1b per charter + ADR disposition).
