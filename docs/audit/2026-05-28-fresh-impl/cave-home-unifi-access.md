# Coverage matrix — cave-home-unifi-access

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · port method spec-based (door/credential engine) + line-by-line (Access API shape) per manifest.
**Verified:** 8/8 mapped symbols found in source · 75 test fns · drift: no.

## MAPPED (implemented + claimed)

| Spec capability | Source symbol | Verified |
|---|---|---|
| Door + hub model: lock state / door-position sensor / tamper / relay | src/door.rs::{AccessDoor,AccessHub,LockState,DoorPosition,RelayState} | yes |
| Lock / unlock door control operations validated against capability | src/control.rs::AccessController::{lock,unlock} | yes |
| Temporary timed unlock with auto-relock after a caller-supplied duration | src/control.rs::AccessController::{temporary_unlock,tick} | yes |
| Evacuation / lockdown house-wide emergency modes | src/control.rs::AccessController::{evacuate,lockdown,clear_emergency} | yes |
| Held-open / door-ajar alarm decision at a closed-should-be threshold | src/control.rs::AccessController::is_held_open | yes |
| Credential model (PIN / NFC card / mobile / wave-to-unlock) with validation | src/credential.rs::Credential::{pin,nfc_card,mobile,wave_to_unlock} | yes |
| Credential secret never leaks via Debug; constant-time digest compare | src/credential.rs::{Credential(Debug impl),CredentialDigest::ct_eq} | yes |
| Brute-force lock-out accounting on enrolled credentials | src/credential.rs::EnrolledCredential::verify | yes |
| Schedule of allowed minute-of-week windows incl. week-wrap | src/schedule.rs::{Schedule,Window} | yes |
| Access-policy decision engine → AccessDecision { granted, reason } | src/policy.rs::Policy::decide | yes |
| Access-event / log model (who, door, granted/denied, tick) + anti-passback hint | src/event.rs::{AccessEvent,AccessLog,AccessLog::is_passback_suspicious} | yes |
| Grandma-friendly localized EN/DE/TR access messages (ADR-007, Charter §6.3) | src/label.rs::AccessMessage::text | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)

| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| UniFi Access REST API transport (doors crawl, lock-rule, control) | phase-1b | Network-bound; maps Ubiquiti Access local Developer REST API (api/v2/developer/doors, set-lock-rule, unlock-now) onto door::AccessDoor + drives control::AccessController. Engine is wire-agnostic. |
| UniFi Access WebSocket live-event subscription | phase-1b | Network-bound; deserialises live device-notifications WebSocket (DPS / lock / access-event changes) into event::AccessEvent + updates door state. Engine is wire-agnostic. |
| Hub / reader hardware protocol (OSDP reader, relay, DPS wiring) | phase-1b | Hardware-bound; physical reader/relay/door-position-sensor protocol behind UniFi Access hub surfaces as door::{LockState,DoorPosition,RelayState} readings. Engine is hardware-agnostic. |
| Real cryptographic credential hashing (Argon2id/scrypt + per-reader salt) | phase-1b | Crypto-bound adapter. Phase-1 CredentialDigest is dependency-free, non-crypto fold owning the contract (validate, digest, constant-time compare, lock-out). Real password-grade KDF swaps in behind same shape. |
| Camera / doorbell pillar tie-in (UniFi Protect doorbell / G4 doorbell) | phase-1b | Cross-crate + network-bound. Access-event model already carries door + actor needed. Tie-in renders through shared camera pillar (cave-home-unifi-protect). |
| cave-home-core entity/state integration + automation triggers | phase-1b | Core-agnostic engine ready. Landing deferred until cave-home-core's entity API stabilises. Engine depends on no other cave-home crate. |
| Ubiquiti cloud / remote-access path | permanent | Charter §9 local-first sovereignty: cave-home talks only to on-prem UniFi Access local API. No Ubiquiti-cloud account or remote-proxy path ever ported. Permanent scope cut, not a deferral. |

## Drift notes

None — every claimed symbol exists in source. All 8 mapped items verified in src/ across {door, control, credential, schedule, policy, event, label}.rs. Custom Debug impl for Credential present (lines 97–104 credential.rs) to prevent secret leak. Constant-time ct_eq at CredentialDigest::ct_eq verified. All spec_test entries (lock/unlock/evacuation/lockdown/held-open/schedule/policy/event/label tests) verified as real test functions. Test count 75 (unit + integration) strongly supports declared coverage. Honest ratio 1.00 is well-supported: fill_ratio=0.30 reflects engine-only Phase 1 scope; every unfilled item carries explicit ADR-009 phase-1b or permanent disposition in unmapped + scope_cut sections (7 items, 6 phase-1b + 1 permanent). No unjustified gaps.
