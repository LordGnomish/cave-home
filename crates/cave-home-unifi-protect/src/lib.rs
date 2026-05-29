// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_const_for_fn)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::doc_markdown)]
#![cfg_attr(
    test,
    allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        clippy::float_cmp
    )
)]
//! `cave-home-unifi-protect` — the UniFi Protect decision brain of the camera
//! pillar (Charter §3 camera pillar, ADR-009 UniFi ecosystem port; the wire-side
//! bootstrap / WebSocket / video transport is deferred to Phase 1b).
//!
//! This crate is the **pure-logic core** of a UniFi Protect integration: it
//! models the Protect devices a household owns (cameras, doorbells, sensors,
//! lights, chimes, viewers), the smart-detection events the NVR emits
//! (person / vehicle / package / animal / face / smoke / CO alarm and the
//! doorbell ring), and the decisions a household actually wants answered —
//! *should this camera be recording right now, did that detection fall in a
//! zone I armed, is this just the same event firing twice, and is this camera
//! masked by a privacy schedule?* It then says it all in plain EN / DE / TR.
//!
//! It deliberately stops at the edge of anything network- or video-bound,
//! because those are exactly the parts that cannot be tested as pure logic.
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here, with **no external crates** (std only):
//! - [`device`] — the typed Protect device models ([`ProtectCamera`],
//!   [`ProtectSensor`], [`ProtectLight`], [`ProtectChime`], [`ProtectViewer`])
//!   with online/offline state and feature flags, plus stable id newtypes.
//! - [`detect`] — the [`SmartDetectType`] taxonomy and the [`DetectionEvent`]
//!   model (camera, detected types, score, start/end tick, thumbnail id).
//! - [`event`] — the doorbell [`RingEvent`] and de-dupe / grouping of rapid
//!   repeat detections of the same kind on the same camera ([`EventGrouper`]).
//! - [`recording`] — the [`RecordMode`] model (Never / Always / Detections /
//!   Schedule) and the [`should_record`] decision.
//! - [`zone`] — named smart-detect [`Zone`]s and line-crossings, each arming a
//!   set of detect-types, and the [`Zone::arms`] decision.
//! - [`privacy`] — the per-camera privacy schedule ([`PrivacySchedule`]) that
//!   masks or disables a camera by time of day (Charter §9).
//! - [`label`] — the grandma-friendly EN / DE / TR phrasing (Charter §6.3,
//!   ADR-007): "Person at the driveway camera", "Doorbell rang at the front
//!   door", "Package detected".
//!
//! # Deferred to Phase 1b (see `parity.manifest.toml` `[[unmapped]]`)
//!
//! The UniFi Protect **REST bootstrap + binary WebSocket update transport**,
//! the **RTSPS stream + recording download**, the **thumbnail / snapshot
//! fetch**, and the **integration with the `cave-home-camera` inference pillar
//! and `cave-home-core`** are all network/video-bound and are enumerated as
//! deferred under ADR-009. They feed this engine (or are driven by its
//! decisions) without changing it. Per Charter §9 there is **no Ubiquiti-cloud
//! dependency** in the critical path — the only supported control surface is
//! the local Protect API, permanently.
//!
//! # Example
//!
//! ```
//! use cave_home_unifi_protect::{
//!     DetectionEvent, Lang, RecordMode, SmartDetectType, Zone, label,
//!     should_record,
//! };
//!
//! // A "driveway" zone on the front camera, armed for people and vehicles.
//! let driveway = Zone::new("driveway")
//!     .arming(SmartDetectType::Person)
//!     .arming(SmartDetectType::Vehicle);
//!
//! // The camera reports a person, confident, at tick 100.
//! let event = DetectionEvent::new("front", 92, 100)
//!     .with_type(SmartDetectType::Person);
//!
//! // The zone is armed for this, so it counts.
//! assert!(driveway.arms(&event));
//!
//! // In "record on detections" mode, an armed detection means: record now.
//! assert!(should_record(RecordMode::Detections, true, false));
//!
//! // And the household sees a plain line, not a WebSocket packet field.
//! let line = label::detection_line(SmartDetectType::Person, "driveway camera", Lang::En);
//! assert_eq!(line, "Person at the driveway camera");
//! ```

pub mod detect;
pub mod device;
pub mod event;
pub mod label;
pub mod privacy;
pub mod recording;
pub mod zone;

pub use detect::{DetectionEvent, SmartDetectType, Tick};
pub use device::{
    CameraId, ChimeId, DeviceState, LightId, ProtectCamera, ProtectChime, ProtectLight,
    ProtectSensor, ProtectViewer, RecordingMode, SensorId, SensorKind, ViewerId,
};
pub use event::{EventGrouper, RingEvent};
pub use label::{detection_line, ring_line, Lang};
pub use privacy::{PrivacyState, PrivacySchedule, TimeOfDay};
pub use recording::{should_record, RecordMode, Schedule};
pub use zone::{LineCrossing, Zone};
