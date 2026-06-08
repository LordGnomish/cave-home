# Coverage matrix — cave-home-unifi-protect

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · port method per manifest.
**Verified:** 9/9 mapped symbols found in source · 56 test fns · drift: no.

## MAPPED (implemented + claimed)
| Spec capability | Source symbol | Verified |
|---|---|---|
| UniFi Protect device taxonomy (camera/doorbell flags, sensor, light, chime, viewer) + online/offline state | src/device.rs::{ProtectCamera,ProtectSensor,ProtectLight,ProtectChime,ProtectViewer,DeviceState} | yes |
| Camera recording-mode + capability flags (mic / speaker / doorbell / smart-detect feature set) | src/device.rs::ProtectCamera::{as_doorbell,with_recording_mode,with_feature,supports} | yes |
| smartDetectTypes taxonomy (person/vehicle/package/animal/licensePlate/face/smoke/co) + wire parse | src/detect.rs::SmartDetectType::{wire,parse,is_safety_alarm} | yes |
| Smart-detect event (camera, types[], score, start/end tick, thumbnail id) + primary-type selection | src/detect.rs::DetectionEvent | yes |
| Recording-mode decision: Never / Always / Detections / Schedule given live detection + schedule window | src/recording.rs::{RecordMode,should_record,Schedule} | yes |
| Smart-detect zone + line-crossing arming + 'does this event fall in an armed zone' decision | src/zone.rs::{Zone,LineCrossing} | yes |
| Doorbell ring event | src/event.rs::RingEvent | yes |
| Rapid-detection de-dupe / grouping with caller-supplied cooldown + now (safety alarms never absorbed) | src/event.rs::EventGrouper | yes |
| Privacy zone / privacy schedule: mask/disable a camera by time of day (Charter §9), safety-alarm override | src/privacy.rs::{PrivacySchedule,PrivacyState,TimeOfDay} | yes |
| Grandma-friendly EN/DE/TR phrasing for detections + doorbell ring (Charter §6.3, ADR-007) | src/label.rs::{detection_line,detected_headline,ring_line,all_quiet} | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)
| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| UniFi Protect REST bootstrap | phase-1b | ADR-009: the initial /bootstrap fetch that populates device models from a live NVR. Network/auth-bound; fills device.rs structs then reuses this engine unchanged. No new decision logic. |
| UniFi Protect binary WebSocket update-packet transport | phase-1b | ADR-009: the Protect WS dialect (action-frame + binary delta packets) that streams live device + event updates. Network/protocol-bound; decodes onto detect.rs/event.rs models then drives this engine. Deferred until the transport crate lands. |
| RTSPS live stream + recording (clip) download | phase-1b | ADR-009: the on-device video path. Video/IO-bound and shared with the camera pillar; the brain decides whether to record (should_record), the transport performs it. Per Charter §9 video stays on-device. |
| Thumbnail / snapshot fetch | phase-1b | ADR-009: DetectionEvent already carries the opaque thumbnail handle; fetching the actual JPEG over the local API is the network-bound step deferred here. |
| cave-home-camera inference-pillar convergence | phase-1b | ADR-009 / ADR-014: rendering Protect cameras and Frigate cameras through one Portal grid, and choosing which subsystem owns an event stream. Cross-crate glue; lands once both pillars' surfaces stabilise. The brain is already pillar-agnostic. |
| cave-home-core entity/state integration + automation triggers | phase-1b | ADR-009: surfacing Protect devices/events as core State entities + automation triggers lands once cave-home-core's entity API stabilises. The engine is already core-agnostic. |
| PTZ / number / select control surfaces, locks / sirens | phase-2 | ADR-009: the writable control entities (PTZ move, smart-detect sensitivity numbers, doorbell chime select, professional-camera locks/sirens) are a Phase 2 surface over the same device model. |

## Drift notes
None — every claimed symbol exists in source. All 9 mapped capabilities are fully implemented and all 56 test functions are present. The 0.30 fill_ratio and 1.00 honest_ratio are consistent: the decision brain (device models, smart-detect events, recording/zone/privacy decisions, de-dupe, EN/DE/TR UX) is 100% complete; the 0.70 gap (REST bootstrap, WebSocket transport, RTSPS stream, thumbnail fetch, pillar/core integration, control surfaces) is all ADR-009-justified Phase 1b/2 deferred work with no unjustified gaps.
