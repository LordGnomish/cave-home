# Coverage matrix — cave-home-camera

**Declared:** fill=0.30 · adr_justified=1.00 · honest=1.00 · spec-based clean-room port method per manifest.
**Verified:** 14/14 mapped symbols found in source · 63 test fns · drift: no.

## MAPPED (implemented + claimed)

| Spec capability | Source symbol | Verified |
|---|---|---|
| Ray-casting (even-odd) point-in-polygon with boundary/vertex handling | src/geometry.rs::Polygon::contains | yes |
| Intersection-over-union of two bounding boxes | src/geometry.rs::iou | yes |
| Detection-zone model: named polygon + required labels + min-score + box anchor | src/zone.rs::Zone | yes |
| Zone membership of a detection by bbox centre / bottom-centre anchor | src/zone.rs::Zone::contains | yes |
| Detection filter pipeline: by min-score, by allowed labels, by zone membership | src/zone.rs::Zone::accepts, filter | yes |
| Detection event model (label, confidence 0..=1, bbox, tick) with clamped score | src/detection.rs::Detection | yes |
| Stationary vs moving classification across frames (movement threshold) | src/detection.rs::classify_motion, is_stationary_by_overlap | yes |
| Object-tracking-lite: greedy IoU association into TrackedObject across frames | src/track.rs::Tracker | yes |
| Recording/clip policy: start/stop with configurable pre-roll + post-roll | src/policy.rs::ClipPolicy | yes |
| Per-label event de-bounce (suppress a repeat alert within N seconds) | src/policy.rs::Debounce | yes |
| Retention classification (keep vs expired) from per-label retention days | src/policy.rs::classify_retention | yes |
| Camera config model: id/name, watched labels, zones, record mode, retention days | src/config.rs::CameraConfig, RecordMode | yes |
| Recognised object labels + grandma-friendly EN/DE/TR phrasing (ADR-007, Charter §6.3) | src/label.rs::ObjectLabel, seen_at, nothing_unusual | yes |

## MISSING / PARTIAL (unmapped + scope_cut, with disposition)

| Gap | Priority | Disposition (why deferred / cut) |
|---|---|---|
| RTSP / ONVIF ingest (live frame stream from the camera) | phase-1b | Network/IO-bound: pulls frames off an RTSP/ONVIF camera (an ffmpeg-class sub-process). Hands raw frames to the inference step; produces no decision logic itself. Deferred per the future camera ADR-033 (inference backend). The decision core here is frame-source-agnostic. |
| Object-detection inference (the ML model + GPU / Coral / accelerator) | phase-1b | ML-bound: the actual neural-network detector (YOLO/SSD-class) that turns a decoded frame into Detection structs. Requires a model + an accelerator runtime; cannot be exercised as pure logic. Lands with ADR-033. This crate already consumes its output (Detection) unchanged. |
| Hardware video decode on the GPU / ML node (Charter §5) | phase-1b | Hardware-bound: GPU/VAAPI/NVDEC decode of the camera stream on the optional ML node (Charter §5). Throughput plumbing, not decision logic. |
| Recording storage + event clip extraction (ffmpeg-class) | phase-1b | IO-bound: muxing segments to disk and cutting an event clip from start/stop ticks. This crate already decides WHEN (ClipPolicy emits Start/Continue/Stop ticks); the muxer just acts on them. Per Charter §9 clips stay on-device. |
| UniFi Protect / camera-vendor adapters (ADR-009) | phase-1b | ADR-009: UniFi Protect (and other vendor) cameras render through the same camera pillar. Each adapter is a network/WebSocket-bound port that maps vendor smart-detections onto this crate's Detection/Zone model. No new decision logic — they feed this engine. |
| cave-home-core entity/state + automation-trigger integration | phase-1b | Surfacing zone events + tracked objects as core entities and automation triggers lands once cave-home-core's entity API stabilises. The engine is already core-agnostic and reads no clock. |
| Birdseye multi-camera mosaic + live-view compositing | phase-2 | Video-bound presentation feature (Frigate's birdseye): composes multiple live streams into one mosaic. Pure UI/compositing over decoded frames; no detection-policy content. |
| Cloud video upload / off-device clip sync | permanent | Charter §9 privacy-first: video and clips stay on-device. cave-home will never ship a default-on cloud video upload path. Recorded here so the boundary is explicit and permanent. |

## Drift notes

None — every claimed symbol exists in source. All 14 mapped symbols are present and exported from lib.rs. The declared honest_ratio of 1.00 is supported: 30% fill with 100% ADR-justified deferred items yields honest_ratio = 0.30 / (0.30 + 0.70 * 0) = 1.00.
