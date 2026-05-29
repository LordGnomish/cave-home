# Coverage matrix — cave-home-free-home

**Declared:** fill=0.00 (scaffold) · adr_justified=N/A · honest=N/A · port method: line-by-line.
**Verified:** 30/30 mapped symbols found in source · 59 test fns (all passing) · drift: YES — manifest is outdated.

## Status

The crate is **fully implemented for Phase 1 MVP scope**. The parity.manifest.toml is a scaffold (fill_ratio=0.00, no [[mapped]]/[[unmapped]] tables) that has not been updated to reflect the actual completed port. The code compiles, all 59 tests pass, and every public symbol in the 8 modules is fully functional.

## MAPPED (implemented + verified in source)

| Module | Spec capability | Source symbol | Verified |
|---|---|---|---|
| **id** | Device serial parsing + format | src/id.rs::DeviceSerial::parse | ✓ |
| **id** | Device serial round-trip | src/id.rs::DeviceSerial::as_str | ✓ |
| **id** | Channel ID parsing + format | src/id.rs::ChannelId::parse | ✓ |
| **id** | Channel ID accessor | src/id.rs::ChannelId::index | ✓ |
| **id** | Datapoint ID parsing + direction | src/id.rs::DatapointId::parse | ✓ |
| **id** | Datapoint ID direction accessor | src/id.rs::DatapointId::direction | ✓ |
| **id** | Direction enum (input/output) | src/id.rs::Direction | ✓ |
| **id** | Parse error handling | src/id.rs::IdError | ✓ |
| **function** | Function ID enum (switch/dimmer/blind/etc.) | src/function.rs::Function | ✓ |
| **function** | Function to ID accessor | src/function.rs::Function::id | ✓ |
| **function** | ID to Function lookup | src/function.rs::Function::from_id | ✓ |
| **function** | Function → DeviceKind mapping | src/function.rs::Function::device_kind | ✓ |
| **function** | Function controllability check | src/function.rs::Function::is_controllable | ✓ |
| **pairing** | Pairing role enum (on/off, brightness, temp, etc.) | src/pairing.rs::Pairing | ✓ |
| **pairing** | Value shape enum (bool/percent/temperature) | src/pairing.rs::ValueShape | ✓ |
| **value** | Value codec (typed variants) | src/value.rs::Value | ✓ |
| **value** | Value decode bool | src/value.rs::Value::decode_bool | ✓ |
| **value** | Value decode percent | src/value.rs::Value::decode_percent | ✓ |
| **value** | Value decode temperature | src/value.rs::Value::decode_temperature | ✓ |
| **value** | Value encode to wire string | src/value.rs::Value::encode | ✓ |
| **value** | Value error type | src/value.rs::ValueError | ✓ |
| **command** | SetDatapoint builder | src/command.rs::SetDatapoint::new | ✓ |
| **command** | SetDatapoint boolean constructor | src/command.rs::SetDatapoint::boolean | ✓ |
| **command** | SetDatapoint percent constructor | src/command.rs::SetDatapoint::percent | ✓ |
| **command** | SetDatapoint temperature constructor | src/command.rs::SetDatapoint::temperature | ✓ |
| **command** | SetDatapoint wire value accessor | src/command.rs::SetDatapoint::wire_value | ✓ |
| **command** | Command error type | src/command.rs::CommandError | ✓ |
| **topology** | SysAp parser (get-all response) | src/topology.rs::SysAp::parse_get_all | ✓ |
| **topology** | Typed tree (SysAp/Device/Channel/Datapoint) | src/topology.rs::{SysAp,Device,Channel,Datapoint} | ✓ |
| **mapping** | DeviceKind projection (Light/Cover/Climate/etc.) | src/mapping.rs::DeviceKind | ✓ |
| **label** | Action phrase generation (EN/DE/TR) | src/label.rs::action_phrase | ✓ |

## MISSING / PARTIAL (unmapped + scope_cut)

| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| SysAP HTTP/REST transport | phase-1b | Network/HTTP binding deferred; cave-home-free-home is pure logic (std-only, no network). Transport sits outside this crate, handled by cave-home-core integration layer (ADR-011). |
| SysAP WebSocket updates | phase-1b | Async/WebSocket binding deferred; Phase 1b scope. |
| Scene programming API | phase-1b | Scene *triggering* mapped; scene *creation* deferred. |
| Timer programming API | phase-1b | Not in Phase 1 MVP scope. |
| KNX-IP bridge tie-in | phase-1b | Cross-crate integration; deferred to Phase 1b. |
| cave-home-core integration | phase-1b | Integration point deferred; library is standalone. |
| CLI runtime wiring | phase-2b | cave-home-cli scaffolds commands but defers to Phase 2b for full integration. |

## Drift notes

**YES — manifest is outdated.** The parity.manifest.toml declares `fill_ratio = 0.00` and `fill_ratio_basis = "scaffold + manifest only (real port lands in phase 1)"`. However, the actual state is:

- All 8 modules (id, function, pairing, value, command, topology, mapping, label) are fully implemented.
- All 30 mapped public symbols exist and are fully functional (verified by grep, cargo build, and test suite).
- 59 unit tests cover the full scope, all passing.
- Crate builds successfully with no errors or warnings.

The manifest is a **scaffold template** that needs updating. The code is **Phase 1 complete** for the declared scope: "domain model + datapoint engine (topology, function/pairing roles, value codec, command validation, device-kind mapping, EN/DE/TR UX)."

**Honest assessment:** fill_ratio should be **1.0** (100% for Phase 1 scope). The deferred items (transport, KNX bridge, core integration) are Phase 1b+, not Phase 1, and correctly documented in lib.rs lines 42-45.
