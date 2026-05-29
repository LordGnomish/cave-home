# Coverage matrix — cave-home-mqtt

**Declared:** fill=0.44 · adr_justified=N/A · honest=N/A (clean-room, spec-derived tests, Phase 1 codec only).
**Verified:** 16/16 mapped symbols found in source · 5 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| MQTT 3.1.1 §2.2.1 Control packet type | src/packet.rs::PacketType | yes |
| MQTT 3.1.1 §2.2.3 Remaining Length variable-byte integer encode | src/codec.rs::encode_var_int | yes |
| MQTT 3.1.1 §2.2.3 Remaining Length variable-byte integer decode | src/codec.rs::decode_var_int | yes |
| MQTT 3.1.1 §1.5.3 UTF-8 encoded string encode | src/codec.rs::encode_str | yes |
| MQTT 3.1.1 §1.5.3 UTF-8 encoded string decode | src/codec.rs::decode_str | yes |
| MQTT 3.1.1 §3.1 CONNECT packet model | src/packet.rs::Connect | yes |
| MQTT 3.1.1 §3.1 CONNECT encode | src/codec.rs::encode_connect | yes |
| MQTT 3.1.1 §3.1 CONNECT decode | src/codec.rs::decode_connect | yes |
| MQTT 3.1.1 §3.2 CONNACK packet model | src/packet.rs::ConnAck | yes |
| MQTT 3.1.1 §3.2.2.3 CONNACK return codes | src/packet.rs::ConnAckReturnCode | yes |
| MQTT 3.1.1 §3.2 CONNACK encode | src/codec.rs::encode_connack | yes |
| MQTT 3.1.1 §3.2 CONNACK decode | src/codec.rs::decode_connack | yes |
| MQTT 3.1.1 §3.3 PUBLISH packet model | src/packet.rs::Publish | yes |
| MQTT 3.1.1 §3.3 PUBLISH encode | src/codec.rs::encode_publish | yes |
| MQTT 3.1.1 §3.3 PUBLISH decode | src/codec.rs::decode_publish | yes |
| MQTT 3.1.1 §4.3 Quality of Service levels | src/packet.rs::QoS | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| SUBSCRIBE / SUBACK / UNSUBSCRIBE / UNSUBACK (§3.8-§3.11) | phase-1b | Lands with topic router; requires topic-filter wildcard matcher |
| PINGREQ / PINGRESP / DISCONNECT (§3.12-§3.14) | phase-1b | Trivial codec-wise; lands with session state machine |
| Retained messages (§3.3.1.3) + Last Will & Testament (§3.1.2.5) | phase-1b | Requires broker session layer; Phase 1 is wire codec only |
| Username / Password / TLS / client cert auth (§3.1.2.8-§3.1.2.9) | phase-1b | Connect flags decoded conservatively (clean-session only) in Phase 1 |
| MQTT 5.0 properties + reason codes + shared subscriptions | phase-2 | Phase 1 is 3.1.1 only; 5.0 is opt-in upgrade path |
| Persistent session storage (in-flight QoS messages on reconnect) | phase-1b | Phase 1 is stateless wire codec |

## Drift notes
None — every claimed symbol exists in source. All 5 spec-derived test functions (designed from OASIS specs, not ported) verified: var_int boundary values, CONNECT round-trip, CONNACK round-trip, PUBLISH QoS 0/1 round-trip, protocol level validation.
