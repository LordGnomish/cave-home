// SPDX-License-Identifier: Apache-2.0
//! Object-detection trait + concrete implementations.
//!
//! Upstream: blakeblackshear/frigate@416a9b7692e052be98ad503704d26c7ef7a4c88d
//! :: frigate/detectors/detection_api.py :: `DetectionApi.detect_raw` and
//! the per-vendor sub-classes (edgetpu_tfl.py, onnx.py, tensorrt.py).
//!
//! Frigate dispatches at config-load time on `detector.type`; we do the
//! same with the `DetectorKind` enum from `crate::config`. The runtime
//! uses the trait, the binary wires the impl.

pub mod coral;
pub mod cpu_yolo;
pub mod mock;
pub mod nvidia;

pub use coral::CoralEdgeTpuDetector;
pub use cpu_yolo::CpuYoloDetector;
pub use mock::MockDetector;
pub use nvidia::NvidiaDetector;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::CameraResult;

/// One detection box, before tracker assignment.
///
/// Port of `frigate.object_detection.detect.RawDetection` (a 6-tuple in
/// Frigate: `(label, score, x, y, w, h)`). We use named fields with
/// normalised f32 corners 0..=1 — the tracker rescales later.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Detection {
    /// Class label ("person", "car", ...).
    pub label: String,
    /// Detector confidence 0..=1.
    pub score: f32,
    /// Box left edge, normalised 0..=1.
    pub x: f32,
    /// Box top edge, normalised 0..=1.
    pub y: f32,
    /// Box width, normalised 0..=1.
    pub w: f32,
    /// Box height, normalised 0..=1.
    pub h: f32,
}

impl Detection {
    /// IoU (intersection-over-union) against another detection.
    /// Used by the tracker.
    #[must_use]
    pub fn iou(&self, other: &Self) -> f32 {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = (self.x + self.w).min(other.x + other.w);
        let y2 = (self.y + self.h).min(other.y + other.h);
        let iw = (x2 - x1).max(0.0);
        let ih = (y2 - y1).max(0.0);
        let inter = iw * ih;
        let area_a = self.w * self.h;
        let area_b = other.w * other.h;
        let union = area_a + area_b - inter;
        if union <= 0.0 { 0.0 } else { inter / union }
    }
}

/// Async detector trait.
///
/// `tensor_input` is a flat row-major RGB8 buffer at the detector's
/// expected input geometry. Frigate's input plumbing has a "model"
/// abstraction that scales the YUV crop to RGB; Phase 1 captures the
/// same idea in `Detector::input_shape` returning the (w, h) the trait
/// expects.
#[async_trait]
pub trait Detector: Send + Sync {
    /// Required RGB input geometry (width, height).
    fn input_shape(&self) -> (u32, u32);

    /// Run one inference and return up to N detections (above the
    /// detector's internal score floor). The returned vector is sorted
    /// by descending score.
    async fn detect(&self, tensor_input: &[u8]) -> CameraResult<Vec<Detection>>;
}
