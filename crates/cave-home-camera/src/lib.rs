// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_const_for_fn)]
#![allow(clippy::map_unwrap_or)]
#![allow(clippy::explicit_iter_loop)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::similar_names)]
#![allow(clippy::redundant_closure_for_method_calls)]
#![allow(clippy::option_if_let_else)]
#![allow(clippy::if_not_else)]
#![allow(clippy::single_match_else)]
#![allow(clippy::format_push_string)]
#![allow(clippy::large_enum_variant)]
#![allow(clippy::or_fun_call)]
#![allow(clippy::redundant_else)]
#![allow(clippy::manual_let_else)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::unused_async)]
#![allow(clippy::case_sensitive_file_extension_comparisons)]
#![allow(clippy::suboptimal_flops)]
#![allow(clippy::wildcard_imports)]
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used, clippy::panic))]
//! cave-home-camera — NVR + object-detection.
//!
//! Line-by-line port of `frigate/` from `blakeblackshear/frigate` v0.17.1
//! (SHA `416a9b7692e052be98ad503704d26c7ef7a4c88d`).
//!
//! Phase 1 MVP scope (per Charter §3 camera pillar):
//! - [`capture`]   — RTSP ingest via `ffmpeg` sub-process; raw frame stream.
//! - [`motion`]    — frame-differencing motion detector (background model).
//! - [`detectors`] — `Detector` trait + 3 production impls
//!   (Coral EdgeTPU, CPU YOLO ONNX, NVIDIA TensorRT)
//!   plus a deterministic `MockDetector` for tests.
//! - [`tracker`]   — IOU-based multi-object tracker across frames.
//! - [`record`]    — MP4 segment writer + event clip extractor.
//! - [`birdseye`]  — multi-camera mosaic composer.
//! - [`events`]    — `CameraEventSink` trait wired to the
//!   `cave-home-automation::EventBus` (Phase 2).
//! - [`config`]    — `CameraConfig` + `Pipeline` settings (YAML mirror).
//! - [`error`]     — top-level error union.
//! - [`prelude`]   — single-import convenience for downstream crates.
//!
//! Out-of-Phase-1 surface (Frigate's audio, MQTT, ONVIF PTZ, web API,
//! review-pipeline, semantic-search, plus-cloud sync) is enumerated in
//! `parity.manifest.toml` `[[unmapped]]`.

pub mod birdseye;
pub mod capture;
pub mod config;
pub mod detectors;
pub mod error;
pub mod events;
pub mod motion;
pub mod prelude;
pub mod record;
pub mod tracker;

pub use config::{CameraConfig, DetectorKind, MotionConfig, RecordConfig};
pub use error::{CameraError, CameraResult};
pub use events::{CameraEvent, CameraEventSink, EventKind, NullEventSink, RecordingEventSink};
