// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_errors_doc)]
#![cfg_attr(
    test,
    allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        clippy::float_cmp
    )
)]
//! `cave-home-camera` — the detection-policy brain of the camera / NVR pillar
//! (Charter §3 camera pillar, ADR-009 camera convergence; inference backend
//! deferred to the future ADR-033).
//!
//! This crate is the **Frigate-class decision core**: it turns a stream of
//! object detections into the answers a household actually wants — *is there a
//! person in the driveway, is it the same person as a moment ago, should we save
//! a clip, and how long do we keep it?* It deliberately stops at the edge of
//! anything ML-, video-, or network-bound, because those are exactly the parts
//! that cannot be tested as pure logic.
//!
//! # Scope (Phase 1 MVP)
//!
//! Implemented, real and tested here, with **no external crates**:
//! - [`geometry`] — points, bounding boxes, a detection [`Polygon`] with a
//!   robust ray-casting point-in-polygon test, and [`iou`] for overlap.
//! - [`label`] — the recognised [`ObjectLabel`] set and the grandma-friendly
//!   EN / DE / TR phrasing (Charter §6.3, ADR-007).
//! - [`detection`] — the [`Detection`] model, the score / label / zone filter
//!   pipeline, and stationary-vs-moving classification.
//! - [`zone`] — a named [`Zone`] (polygon + required labels + min score) and the
//!   accept/filter logic.
//! - [`track`] — object-tracking-lite: a greedy `IoU` [`Tracker`] that stitches
//!   per-frame detections into persistent [`TrackedObject`]s.
//! - [`config`] — the per-camera [`CameraConfig`] (id / name / watched labels /
//!   zones / [`RecordMode`] / retention days).
//! - [`policy`] — the [`ClipPolicy`] (pre/post-roll start-stop), the per-label
//!   [`Debounce`] and the [`classify_retention`] rule.
//!
//! # Deferred to Phase 1b (see `parity.manifest.toml` `[[unmapped]]`)
//!
//! The **RTSP / ONVIF ingest**, the **object-detection inference** (the ML model
//! plus its GPU / Coral / accelerator), **hardware decode** (the Charter §5 ML
//! node), **recording storage + clip extraction** (ffmpeg-class), the **`UniFi`
//! Protect / camera-vendor adapters** (ADR-009) and **cave-home-core
//! integration** are all ML / video / IO / network-bound and are enumerated as
//! deferred. They feed this engine (or are driven by its decisions) without
//! changing it. Per Charter §9 there is **no cloud video upload** — video stays
//! on-device, permanently.
//!
//! # Example
//!
//! ```
//! use cave_home_camera::{
//!     BBox, CameraConfig, ClipAction, ClipPolicy, Detection, Lang, ObjectLabel,
//!     Point, Polygon, RecordMode, Zone, label,
//! };
//!
//! // The household draws a "driveway" box over the camera view and says it
//! // cares about people and cars there, confident at 0.6+.
//! let driveway = Zone::new(
//!     "driveway",
//!     Polygon::new(vec![
//!         Point::new(0.0, 0.0),
//!         Point::new(100.0, 0.0),
//!         Point::new(100.0, 100.0),
//!         Point::new(0.0, 100.0),
//!     ]).unwrap(),
//!     vec![ObjectLabel::Person, ObjectLabel::Car],
//!     0.6,
//! );
//! let cam = CameraConfig::new("front", "Front camera")
//!     .with_zone(driveway.clone())
//!     .with_record_mode(RecordMode::MotionOnly)
//!     .with_retention_days(7);
//!
//! // A car is detected, bottom-centre inside the driveway, confidently.
//! let car = Detection::new(ObjectLabel::Car, 0.82, BBox::new(40.0, 60.0, 20.0, 30.0), 100);
//! assert!(driveway.accepts(&car));
//!
//! // That activity starts an event clip, padded by a 5s pre-roll.
//! let mut clip = ClipPolicy::new(5, 10);
//! assert_eq!(clip.observe(true, 100), ClipAction::Start(95));
//!
//! // And the household sees a plain line, not a model class index.
//! let place = driveway.friendly_name(Lang::En);
//! assert_eq!(label::seen_at(ObjectLabel::Car, &place, Lang::En), "Car at the driveway");
//! # let _ = cam.retention_days();
//! ```

pub mod config;
pub mod detection;
pub mod geometry;
pub mod label;
pub mod policy;
pub mod track;
pub mod zone;

pub use config::{CameraConfig, RecordMode};
pub use detection::{
    classify_motion, is_stationary_by_overlap, Detection, Motion, Tick, ZoneAnchor,
};
pub use geometry::{iou, BBox, Point, Polygon, PolygonError};
pub use label::{nothing_unusual, seen_at, Lang, ObjectLabel};
pub use policy::{
    classify_retention, ClipAction, ClipPolicy, Debounce, Retention, SECONDS_PER_DAY,
};
pub use track::{TrackId, TrackedObject, Tracker};
pub use zone::{filter, Zone};
