# Coverage matrix — cave-home-unifi-talk

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · port method spec-based (clean-room call-control domain model).
**Verified:** 11/11 mapped symbols found in source · 76 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| Device + extension roster model (desk-phone / intercom / doorbell, online/offline) | src/device.rs::TalkDevice, src/device.rs::DeviceKind, src/device.rs::DeviceState | yes |
| Extension (number, display name, assigned device, voicemail) + dialable-number validation | src/extension.rs::Extension | yes |
| Ring group + ring strategy (ring-all / sequential / round-robin) ordering | src/extension.rs::CallGroup, src/extension.rs::RingStrategy | yes |
| Call lifecycle state set (Idle/Ringing/Connecting/Active/Held/Ended/Missed/Voicemail) | src/call.rs::CallState | yes |
| Call event vocabulary + valid/invalid transition handling | src/call.rs::CallEvent, src/call.rs::CallMachine | yes |
| Ring-no-answer timeout → Missed / Voicemail per configured disposition | src/call.rs::CallMachine::tick, src/call.rs::CallMachine::apply | yes |
| Blind / attended transfer + hold / resume model | src/call.rs::TransferKind, src/call.rs::CallMachine::apply | yes |
| Three-way / conference membership model (join/leave, size cap) | src/call.rs::Conference | yes |
| Time-based routing schedule (business hours, midnight-wrapping; after-hours → voicemail/forward) | src/schedule.rs::BusinessHours | yes |
| Incoming-call routing: who rings, in what order, honouring per-extension DND + emergency override + call-forwarding | src/routing.rs::route_call, src/routing.rs::route_group, src/routing.rs::RoutingPrefs, src/routing.rs::RouteOutcome | yes |
| Call-log / history model (from, to, direction, outcome, duration, tick) | src/log.rs::CallRecord, src/log.rs::CallLog, src/log.rs::CallDirection | yes |
| Grandma-friendly EN/DE/TR call lines + DND status (ADR-007, Charter §6.3) | src/label.rs::for_state, src/label.rs::incoming_from_device, src/label.rs::missed_from, src/label.rs::do_not_disturb_on | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| SIP signalling + RTP media transport + audio codecs (G.711/G.722/Opus) | phase-1b | ADR-009: actual two-way audio path is signalling/media-bound; engine produces RouteOutcome + drives CallMachine; SIP/RTP stack carries media beneath without changing call-control logic |
| UniFi Talk provisioning REST/WS API client | phase-1b | ADR-009 §4: Ubiquiti's Talk API under-documented; parity ceiling = whatever it exposes; network-bound I/O adapter mapping controller's device/extension list onto domain model |
| Voicemail recording, storage + playback | phase-1b | ADR-009: engine decides call rolls to Voicemail; capturing/storing/playing recording is audio + storage surface landing with SIP/RTP transport |
| PSTN / SIP-trunk integration (calls to/from public phone network) | phase-1b | ADR-009: trunking to external SIP provider is transport concern layered under same routing engine; routing model already treats external number as forward target |
| cave-home-doorbell / cave-home-core integration glue | phase-1b | ADR-009: surfacing doorbell press as incoming intercom call + call events as core automation triggers lands once those crates' event APIs stabilise; engine is crate-agnostic, depends on no other cave-home crate |
| Cloud-VoIP relay / hosted-Talk dependency | permanent | Charter §9 local-first / no cloud dependency in critical path: cave-home routes and rings on local network with no internet access; will never require cloud-VoIP relay; cloud SIP trunking stays opt-in augmentation only |
| Legacy / pre-current UniFi Talk API compatibility shim | permanent | Charter §7 always-latest + §8 no-backcompat: cave-home tracks current Ubiquiti Talk surface only; no historical-API compatibility mode |

## Drift notes
None — every claimed symbol exists in source. Declared honest_ratio (1.00) is fully supported: the 30% fill (call-control engine) is comprehensively implemented and tested across 76 test functions; the 70% gap (transport/API/audio/integration) is entirely ADR-justified Phase 1b/permanent deferred work with clear disposition.
