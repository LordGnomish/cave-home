# Coverage matrix — cave-home-knx

**Declared:** fill=0.35 · adr_justified=1.00 · honest=1.00 · port method: spec-based (public KNX DPT/APCI tables) + xknx-MIT public-behavior reference; knxd clean-room (deferred).
**Verified:** 17/17 mapped symbols found in source · 49 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| KNX group address — 3-level / 2-level / free notation, parse ⇄ raw u16 ⇄ string, range validation | src/address.rs::GroupAddress | yes |
| KNX individual address area.line.device (4/4/8 bits), parse ⇄ raw ⇄ string | src/address.rs::IndividualAddress | yes |
| DPT 1.x — 1-bit boolean (switch / up-down / open-close) | src/dpt.rs::dpt1 | yes |
| DPT 2.x — 1-bit value + control flag (2-bit field) | src/dpt.rs::dpt2 | yes |
| DPT 3.x — 4-bit dimming / blind control (direction + step code) | src/dpt.rs::dpt3 | yes |
| DPT 5.x — 1-byte unsigned: scaling 0..=100 %, angle 0..=360°, raw (round-half-up) | src/dpt.rs::dpt5 | yes |
| DPT 6.x — 1-byte signed (two's complement) | src/dpt.rs::dpt6 | yes |
| DPT 7.x / 8.x — 2-byte unsigned / signed, big-endian | src/dpt.rs::dpt7, dpt8 | yes |
| DPT 9.x — KNX 2-byte float (sign, 4-bit exponent, 11-bit signed mantissa; value = 0.01·M·2^E) | src/dpt.rs::dpt9 | yes |
| DPT 12.x / 13.x — 4-byte unsigned / signed, big-endian | src/dpt.rs::dpt12, dpt13 | yes |
| DPT 14.x — 4-byte IEEE-754 single-precision float, big-endian | src/dpt.rs::dpt14 | yes |
| DPT 16.x — 14-byte fixed-width string, null-padded | src/dpt.rs::dpt16 | yes |
| Application services — GroupValueRead / Write / Response APCI codes | src/apci.rs::GroupService | yes |
| Group telegram APDU framing incl. ≤6-bit small-payload optimization | src/telegram.rs::GroupTelegram | yes |
| Grandma-friendly EN/DE/TR action descriptions (light/blinds/temperature, ADR-007 / Charter §6.3) | src/label.rs::Action | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| KNXnet/IP tunneling + routing transport (UDP wire protocol) | phase-1b | ADR-011: the KNXnet/IP service-code framing (CONNECT/TUNNELING_REQUEST, routing multicast 224.0.23.12) carries the telegrams this codec already encodes. Network-bound (UDP sockets); adds no datapoint logic — it wraps src/telegram.rs. |
| USB / TPUART serial transport (KNX-TP1 bus access) | phase-1b | ADR-011: the TPUART serial framing for direct twisted-pair bus access. Hardware-bound (serial device); moves the same telegram bytes this crate produces. |
| knxd-equivalent gateway daemon (clean-room) | phase-1b | ADR-011 / Charter §6.1: a KNXnet/IP routing daemon built EXCLUSIVELY from the KNX Association public service-code table. KNXd is GPL-3.0 and its source is NOT read. Network-bound service; deferred until the transport lands. |
| ETS project import (group-address ⇄ DPT mapping) | phase-1b | ADR-011: importing an installer's ETS export to learn which group address speaks which DPT. File-format/parser work that configures, but is not part of, this pure codec. |
| cave-home-core entity / automation integration | phase-1b | ADR-011: surfacing KNX groups as core State entities + automation triggers lands once cave-home-core's entity API stabilises. The codec is already core-agnostic. |
| Legacy / pre-current KNX DPT snapshot compatibility mode | permanent | Charter §7 always-latest + §8 no-backcompat: cave-home ships the current public KNX DPT encodings only; no historical-snapshot or legacy compatibility mode. |

## Drift notes
None — every claimed symbol exists in source. All 17 mapped symbols verified in source code. Fill ratio (0.35) correctly reflects codec-only implementation vs. full KNX stack scope. All 6 unmapped items carry explicit ADR-011 phase disposition and are justified. Honest ratio (1.00) is supported: 0.35 codec fill / (0.35 + 0 unjustified gap) = 100% honest allocation of deferred work.
