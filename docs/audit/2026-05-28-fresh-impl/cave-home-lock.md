# Coverage matrix — cave-home-lock

**Declared:** fill=0.28 · adr_justified=1.00 · honest=1.00 · port method: spec-based (lock state machine) + first-party (safety/PIN logic).
**Verified:** 12/12 mapped symbols found in source · 40 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| HA lock states: locked / unlocked / locking / unlocking / jammed / open / unknown | src/state.rs::LockState | yes |
| HA lock commands: lock / unlock / open | src/state.rs::LockCommand | yes |
| Optimistic transition: lock/unlock enter transient in-flight state | src/machine.rs::Lock::apply | yes |
| Confirm/fail settling: transient→settled or→Jammed on failure | src/machine.rs::Lock::{confirm,fail} | yes |
| Jam handling: jammed lock rejects software commands until cleared | src/machine.rs::Lock::{is_legal,clear_jam} | yes |
| LockEntityFeature.OPEN capability gating (reject Open when unsupported) | src/machine.rs::{LockFeatures,Lock::is_legal} | yes |
| Illegal-transition rejection (redundant in-flight, jammed lock) | src/machine.rs::TransitionError | yes |
| Keypad PIN value object: validated, non-empty, length-bounded, digits-only | src/code.rs::LockCode | yes |
| No-plaintext credential contract: opaque digest + constant-time compare | src/code.rs::{CodeDigest,CodeCredential::verify} | yes |
| Keypad brute-force lock-out after N consecutive failures | src/code.rs::CodeCredential | yes |
| Grandma-friendly EN/DE/TR status label & advice per state (Charter §6.3) | src/label.rs::LockState::{label,advice} | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| Nuki adapter (HA integration + Nuki Smart Lock REST API) | phase-1b | ADR-016: Network-bound I/O adapter; maps vendor states onto state machine unchanged. Local API preferred; cloud trade-off surfaced in UI. |
| SwitchBot Lock adapter (HA integration + SwitchBot Lock API) | phase-1b | ADR-016: Hardware/account-bound I/O adapter; BLE or cloud API; routes vendor states into state machine. No new lock logic. |
| August / Yale adapter | phase-1b | ADR-016: Cloud/account-bound I/O adapter; residential locks via HA integration; binds onto state machine. |
| Aqara lock adapter | phase-1b | ADR-016: Hardware-bound via hub/Zigbee; binds lock-domain entity onto state machine. |
| ESPHome lock-component bindings | phase-1b | ADR-016: Custom locks with lock components; adapter adds lock-domain entity bindings; wire transport via ESPHome adapter. |
| Zigbee / Z-Wave / Matter door-lock bindings | phase-1b | ADR-016: Protocol-pillar locks via existing protocol crates; this crate supplies lock-domain abstraction. Radio/protocol-bound. |
| Real cryptographic PIN hashing (salted Argon2id/scrypt, constant-time crate) | phase-1b | ADR-016: Phase-1 digest is deliberately dependency-free fold, NOT password-grade. Phase-1b swaps in salted KDF + constant-time primitive behind same contract. |
| Captured-trace vendor integration-test fixtures | phase-1b | ADR-016 accepted-costs: Safety-critical lock state requires integration tests vs. captured traces. Fixtures arrive with each vendor adapter (Phase 1b); engine unit-tested in place. |
| Legacy lock-state compatibility / pre-current HA state names | permanent | Charter §7 (always-latest) + §8 (no-backcompat): models current HA lock-domain states only; no historical snapshots or legacy names. |

## Drift notes
None — every claimed symbol exists in source.
