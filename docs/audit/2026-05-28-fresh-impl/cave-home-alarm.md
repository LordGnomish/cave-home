# Coverage matrix — cave-home-alarm

**Declared:** fill=0.27 · adr_justified=1.00 · honest=1.00 · port method: spec-based (HA alarm_control_panel entity domain) + first-party safety logic.
**Verified:** 11/11 mapped symbols found in source · 59 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| HA alarm states: disarmed / armed_home / armed_away / armed_night / armed_vacation / armed_custom_bypass / arming / pending / triggered | src/state.rs::AlarmState | yes |
| HA alarm commands: disarm / arm_home / arm_away / arm_night / arm_vacation / trigger | src/state.rs::AlarmCommand | yes |
| Exit-delay arming: transient Arming state then requested armed state once exit delay elapses | src/machine.rs::AlarmPanel::{apply_with_code,apply,tick} | yes |
| Entry-delay: watched sensor trip while armed enters Pending, then Triggered if not disarmed after entry delay | src/machine.rs::AlarmPanel::{sensor_trip,tick} | yes |
| Instant zone: armed_home / armed_night sound alarm immediately, skipping entry delay | src/machine.rs::AlarmPanel::{zone_is_instant,sensor_trip} | yes |
| Siren time: Triggered persists for configured trigger_time, then auto-returns to prior armed state | src/machine.rs::AlarmPanel::tick | yes |
| Disarm always requires valid code; arming code-optional per config (HA CODE_ARM_REQUIRED) | src/machine.rs::AlarmPanel::{command_requires_code,apply,apply_with_code} | yes |
| Illegal-transition rejection (re-arm same mode, arm/trigger while pending/triggered, disarm-while-disarmed) | src/machine.rs::{AlarmError,AlarmPanel::check_legal} | yes |
| Validated per-panel timing/policy config (exit/entry/trigger delays, code-on-arm, instant flags), rejects silent-alarm & out-of-range | src/config.rs::{PanelConfig,ConfigError} | yes |
| User-code value object: validated, non-empty, length-bounded, digits-only, never leaks via Debug | src/code.rs::UserCode | yes |
| No-plaintext credential contract: opaque digest + constant-time compare | src/code.rs::{CodeDigest,CodeCredential::verify} | yes |
| Keypad brute-force lock-out after N consecutive failures (refuses even correct code once locked out) | src/code.rs::CodeCredential | yes |
| Grandma-friendly EN/DE/TR status label & advice per state (Charter §6.3, ADR-007) | src/label.rs::AlarmState::{label,advice} | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| Door/window/motion sensor & zone hardware adapters (Zigbee / Z-Wave / MQTT) | phase-1b | ADR-018: adapters map hardware trip onto machine::Zone + AlarmPanel::sensor_trip; radio/hardware-bound I/O |
| Siren / strobe actuator output | phase-1b | ADR-018: Phase-1b actuator adapter drives physical siren/strobe when engine enters Triggered; machine owns timing |
| AlarmDecoder adapter (Honeywell / DSC wired panels over USB/network) | phase-1b | ADR-018: adapter maps panel state/keypad events onto state::AlarmState + machine::AlarmPanel; hardware-bound I/O |
| Per-panel vendor integrations (Bosch, ELK-M1, and other HA alarm panels) | phase-1b | ADR-018: modern panel-class integrations bind vendor state/command set onto this domain abstraction; network/hardware-bound |
| Real cryptographic code hashing (salted Argon2id/scrypt, vetted constant-time crate) | phase-1b | ADR-018: Phase-1 digest in code::CodeDigest is dependency-free fold, not password-grade; Phase-1b swaps in salted KDF |
| cave-home-core event-bus / entity-state integration + automation triggers | phase-1b | ADR-018: surfacing panel as core State entity, emitting arm/disarm/trigger events; state machine already core-agnostic |
| Third-party alarm-monitoring relay | permanent | ADR-018 + Charter §9: cave-home is single trust boundary; no third-party monitored-service relay; users integrate via notify pillar |
| Legacy alarm-state compatibility / pre-current HA state names | permanent | Charter §7 always-latest + §8 no-backcompat: current HA alarm_control_panel state set only; no historical snapshot mode |

## Drift notes
None — every claimed symbol exists in source.
