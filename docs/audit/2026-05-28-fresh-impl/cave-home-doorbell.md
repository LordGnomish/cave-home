# Coverage matrix — cave-home-doorbell

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · port method spec-based (call engine) + line-by-line (vendor adapters, phase-1b).
**Verified:** 10/10 mapped symbols found in source · 56 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| Call states Idle/Ringing/Answered/Missed/Declined/Ended + active/terminal predicates | src/event.rs::CallState | yes |
| Doorbell event vocabulary (button/motion/answer/decline/end/timeout) | src/event.rs::DoorbellEvent | yes |
| Call state machine: press/motion→ringing, answer→answered, decline→declined, answered+end→ended; illegal-transition rejection | src/machine.rs::CallMachine::apply | yes |
| Ring timeout → Missed at the caller-supplied boundary (clock-free tick) | src/machine.rs::CallMachine::tick | yes |
| Indoor chime tone selection + do-not-disturb + per-event enable | src/chime.rs::ChimePolicy::decide | yes |
| Quiet-hours gating with midnight-wrapping half-open [start,end) window | src/chime.rs::ChimePolicy::in_quiet_hours | yes |
| Motion/ring de-dup + cooldown collapse over a caller-supplied last-accepted tick | src/cooldown.rs::dedup | yes |
| Snapshot-vs-clip media request model (kind + reason + tick) for the camera pillar | src/media.rs::MediaRequest::for_event | yes |
| Visitor-log entry model (event + tick + outcome) + bounded history | src/log.rs::VisitorLog | yes |
| Grandma-friendly EN/DE/TR front-door notification lines (Charter §6.3, ADR-007) | src/label.rs | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| Reolink doorbell adapter | phase-1b | ADR-018: Local HTTP API + RTSP stream and button-press webhook. Network-bound; maps press/motion edges onto DoorbellEvent. I/O adapter only. |
| DoorBird intercom adapter | phase-1b | ADR-018: HTTP/REST + RTSP public API. Network-bound; surfaces press/motion and renders two-way audio separately. I/O + media-transport adapter. |
| Amcrest doorbell adapter | phase-1b | ADR-018: Local HTTP/RTSP. Network-bound button/motion source feeding DoorbellEvent. I/O adapter only. |
| UniFi Talk-class intercom adapter | phase-1b | ADR-018 + ADR-009 (camera pillar): Network-bound; press/motion events route into engine, video via camera pillar. |
| Ring doorbell adapter (RTSP-only) | phase-1b | ADR-018 + Charter §9: RTSP-only, NO Ring cloud-account integration. Local-stream-bound adapter feeding press/motion. |
| Aqara doorbell adapter | phase-1b | ADR-018: Over Zigbee bridge. Hardware-bound; routes press/motion edge into DoorbellEvent. I/O adapter only. |
| Two-way SIP/WebRTC intercom audio | phase-1b | ADR-018: Live two-way audio in Answered call is a SIP/WebRTC media session. Network/codec-bound; orthogonal to call-state engine. |
| Camera-pillar snapshot/clip capture | phase-1b | ADR-018 + ADR-009: This crate emits MediaRequest; actual frame grab is the camera pillar's job. Stream/transport-bound; deliberately decoupled. |
| cave-home-core event-bus integration | phase-1b | ADR-018: Surfacing call states + media requests as core events lands once cave-home-core's entity/event API stabilises. Engine is core-agnostic. |
| Legacy/vendor-snapshot doorbell compatibility modes | permanent | Charter §7 + §8: Always-latest + no-backcompat; ships current interaction model only; no historical-firmware compatibility shims. |

## Drift notes
None — every claimed symbol exists in source. All 10 mapped specifications are implemented as specified. The fill_ratio=0.30 reflects that the Phase 1 MVP (call engine logic: state machine, ring timeout, chime policy, de-dup, media requests, visitor log, UI labels) is complete and tested; the six vendor I/O adapters, two-way audio, camera capture, and core integration are Phase-1b network/hardware-bound and ADR-018-justified. honest_ratio=1.00 because every unfilled gap in the spec carries an explicit ADR-018 or Charter disposition — no unjustified gaps exist.
