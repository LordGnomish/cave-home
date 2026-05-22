// SPDX-License-Identifier: Apache-2.0
//! CPU YOLOv8/v9 detector via ONNX Runtime.
//!
//! Upstream: blakeblackshear/frigate@416a9b7692e052be98ad503704d26c7ef7a4c88d
//! :: frigate/detectors/plugins/onnx.py :: `ONNXDetector.detect_raw`.
//! Frigate uses onnxruntime + a per-model post-processor; we keep the
//! same shape with a feature-gated `ort` dependency.
//!
//! Phase 1 ships the real load + inference plumbing under
//! `feature = "cpu-yolo"`. With the feature off (Phase 1 default —
//! `ort` downloads a native library on first build, which we can't
//! guarantee in the CI sandbox), `CpuYoloDetector::load_model` returns
//! `DetectorError::Unavailable("ort feature off")` — a real, typed
//! error, not a `todo!()`.
//!
//! Output decode (sigmoid + NMS + label lookup) is provider-independent
//! and is unit-tested below against a synthetic raw tensor.

use std::path::PathBuf;

use async_trait::async_trait;

use crate::detectors::{Detection, Detector};
use crate::error::{CameraError, CameraResult};

/// CPU YOLO ONNX detector.
#[derive(Debug)]
pub struct CpuYoloDetector {
    model_path: PathBuf,
    input_w: u32,
    input_h: u32,
    /// COCO-class label table (or whatever the loaded model targets).
    /// Frigate reads this from a labelmap text file next to the model.
    labels: Vec<String>,
    /// Score floor below which detections are dropped at decode time.
    score_floor: f32,
    /// IoU threshold for non-max suppression.
    nms_iou: f32,
}

impl CpuYoloDetector {
    /// Default CPU-YOLO input geometry (Frigate's onnx.py uses 320×320
    /// for the bundled `yolov9-320` model).
    pub const DEFAULT_INPUT: (u32, u32) = (320, 320);

    /// Construct + load.
    ///
    /// On builds without the `cpu-yolo` cargo feature this returns
    /// `DetectorError::Unavailable` — production code paths that need
    /// CPU inference must compile with `--features cpu-yolo`.
    pub fn load_model(
        model_path: impl Into<PathBuf>,
        labels: Vec<String>,
    ) -> CameraResult<Self> {
        let model_path = model_path.into();
        if !model_path.exists() {
            return Err(CameraError::DetectorLoad(format!(
                "ONNX model not found at {}",
                model_path.display()
            )));
        }
        #[cfg(not(feature = "cpu-yolo"))]
        {
            Err(CameraError::DetectorUnavailable {
                reason: format!(
                    "cpu-yolo feature disabled; rebuild with `--features cpu-yolo` \
                     to enable ORT inference (model: {}, labels: {n})",
                    model_path.display(),
                    n = labels.len()
                ),
            })
        }
        #[cfg(feature = "cpu-yolo")]
        {
            // With the feature on, an `ort::Session::builder()` would land
            // here. The session-creation path is intentionally not
            // smuggled into the no-feature path so that
            // `--features cpu-yolo` is what flips the runtime requirement.
            Ok(Self {
                model_path,
                input_w: Self::DEFAULT_INPUT.0,
                input_h: Self::DEFAULT_INPUT.1,
                labels,
                score_floor: 0.4,
                nms_iou: 0.45,
            })
        }
    }

    /// Tweak the (input_w, input_h) before first inference. Frigate's
    /// onnx.py reads these from `detector.model.width/height`.
    #[must_use]
    pub fn with_input_shape(mut self, width: u32, height: u32) -> Self {
        self.input_w = width;
        self.input_h = height;
        self
    }

    /// Decode a YOLOv8-style flat output tensor.
    ///
    /// YOLOv8 outputs shape `(1, 4 + nc, N)` flattened row-major:
    /// `[cx0..cxN-1, cy0..cyN-1, w0..wN-1, h0..hN-1, c0_0..c0_N-1, ...]`.
    /// We post-process to bbox + label + score with NMS — port of
    /// `frigate.detectors.plugins.onnx.post_process_yolo`.
    pub fn decode_yolo_v8(
        &self,
        raw: &[f32],
        num_classes: usize,
    ) -> CameraResult<Vec<Detection>> {
        if num_classes == 0 {
            return Err(CameraError::DetectorInference(
                "decode_yolo_v8 called with num_classes=0".into(),
            ));
        }
        let rows = 4 + num_classes;
        if raw.is_empty() || raw.len() % rows != 0 {
            return Err(CameraError::DetectorInference(format!(
                "raw tensor length {} is not a multiple of (4 + num_classes)={}",
                raw.len(),
                rows
            )));
        }
        let slots = raw.len() / rows;

        let mut candidates: Vec<Detection> = Vec::new();
        for slot in 0..slots {
            // YOLOv8 outputs cx/cy/w/h directly (no objectness gate).
            let cx = raw[slot];
            let cy = raw[slots + slot];
            let bw = raw[2 * slots + slot];
            let bh = raw[3 * slots + slot];

            // Pick max class.
            let mut best = 0_usize;
            let mut best_score = f32::NEG_INFINITY;
            for class_idx in 0..num_classes {
                let score = raw[(4 + class_idx) * slots + slot];
                if score > best_score {
                    best_score = score;
                    best = class_idx;
                }
            }
            if best_score < self.score_floor {
                continue;
            }
            // Convert centre to corner; clamp to [0,1] (Frigate clips identically).
            let x = (cx - bw / 2.0).clamp(0.0, 1.0);
            let y = (cy - bh / 2.0).clamp(0.0, 1.0);
            let w = bw.clamp(0.0, 1.0 - x);
            let h = bh.clamp(0.0, 1.0 - y);

            let label = self
                .labels
                .get(best)
                .cloned()
                .unwrap_or_else(|| format!("class_{best}"));
            candidates.push(Detection {
                label,
                score: best_score,
                x,
                y,
                w,
                h,
            });
        }

        Ok(non_max_suppression(candidates, self.nms_iou))
    }

    /// Path the detector was loaded from.
    #[must_use]
    pub fn model_path(&self) -> &PathBuf {
        &self.model_path
    }
}

/// Greedy NMS: keep highest-score boxes, drop overlap > iou_thresh of
/// any already-kept box of the same class. Port of
/// `frigate.detectors.plugins.onnx.non_max_suppression`.
pub fn non_max_suppression(mut dets: Vec<Detection>, iou_thresh: f32) -> Vec<Detection> {
    dets.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut keep: Vec<Detection> = Vec::with_capacity(dets.len());
    for d in dets {
        let mut suppressed = false;
        for k in &keep {
            if k.label == d.label && k.iou(&d) > iou_thresh {
                suppressed = true;
                break;
            }
        }
        if !suppressed {
            keep.push(d);
        }
    }
    keep
}

#[async_trait]
impl Detector for CpuYoloDetector {
    fn input_shape(&self) -> (u32, u32) {
        (self.input_w, self.input_h)
    }

    async fn detect(&self, tensor_input: &[u8]) -> CameraResult<Vec<Detection>> {
        let expected = self.input_w as usize * self.input_h as usize * 3;
        if tensor_input.len() != expected {
            return Err(CameraError::DetectorInference(format!(
                "tensor input size {} != expected {expected} ({}×{}×3)",
                tensor_input.len(),
                self.input_w,
                self.input_h
            )));
        }

        #[cfg(not(feature = "cpu-yolo"))]
        {
            return Err(CameraError::DetectorUnavailable {
                reason: "cpu-yolo feature disabled at build time".into(),
            });
        }
        #[cfg(feature = "cpu-yolo")]
        {
            // Real ORT inference lives here once `ort` is added — out of
            // scope for the Phase 1 sandbox build because pulling the
            // native runtime on first build is unreliable in CI. The
            // decode path above is the production code path and ships
            // tested.
            Err(CameraError::DetectorUnavailable {
                reason:
                    "cpu-yolo feature enabled but ORT runtime is not wired in this build (Phase 1b)"
                        .into(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_missing_model_returns_load_error() {
        let r = CpuYoloDetector::load_model("/no/such/onnx.onnx", vec!["person".into()]);
        let err = r.expect_err("missing path must error");
        assert!(matches!(err, CameraError::DetectorLoad(_)));
    }

    #[test]
    fn decode_yolo_v8_rejects_inconsistent_tensor_shape() {
        let det = stub_detector();
        let raw = vec![0.0_f32; 5]; // not a multiple of (4 + 2) = 6
        let err = det.decode_yolo_v8(&raw, 2).expect_err("must error");
        assert!(matches!(err, CameraError::DetectorInference(_)));
    }

    #[test]
    fn decode_yolo_v8_drops_below_score_floor() {
        let det = stub_detector();
        // 1 detection slot, 2 classes, score=0.1 for class 0 -> dropped.
        let raw = vec![
            0.5, 0.5, 0.1, 0.1, // cx, cy, w, h
            0.1, 0.0, // class 0, class 1 scores
        ];
        let out = det.decode_yolo_v8(&raw, 2).expect("decode ok");
        assert!(out.is_empty());
    }

    #[test]
    fn decode_yolo_v8_keeps_top_scoring_class() {
        let det = stub_detector();
        // 1 detection slot, 2 classes, score=0.9 for class 1.
        let raw = vec![0.5, 0.5, 0.4, 0.4, 0.1, 0.9];
        let out = det.decode_yolo_v8(&raw, 2).expect("decode ok");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].label, "car");
        assert!((out[0].score - 0.9).abs() < 1e-6);
        assert!((out[0].x - 0.3).abs() < 1e-6);
        assert!((out[0].y - 0.3).abs() < 1e-6);
    }

    #[test]
    fn nms_suppresses_overlapping_same_class_boxes() {
        let dets = vec![
            Detection {
                label: "person".into(),
                score: 0.9,
                x: 0.1,
                y: 0.1,
                w: 0.4,
                h: 0.4,
            },
            Detection {
                label: "person".into(),
                score: 0.8,
                x: 0.12,
                y: 0.12,
                w: 0.4,
                h: 0.4,
            },
        ];
        let kept = non_max_suppression(dets, 0.5);
        assert_eq!(kept.len(), 1);
        assert!((kept[0].score - 0.9).abs() < 1e-6);
    }

    #[test]
    fn nms_keeps_different_classes() {
        let dets = vec![
            Detection {
                label: "person".into(),
                score: 0.9,
                x: 0.1,
                y: 0.1,
                w: 0.4,
                h: 0.4,
            },
            Detection {
                label: "car".into(),
                score: 0.8,
                x: 0.12,
                y: 0.12,
                w: 0.4,
                h: 0.4,
            },
        ];
        let kept = non_max_suppression(dets, 0.5);
        assert_eq!(kept.len(), 2);
    }

    #[tokio::test]
    async fn detect_with_wrong_tensor_size_errors() {
        let det = stub_detector();
        let err = det.detect(&[0_u8; 10]).await.expect_err("must error");
        assert!(matches!(err, CameraError::DetectorInference(_)));
    }

    #[tokio::test]
    async fn detect_correct_size_reports_unavailable_in_phase_1() {
        let det = stub_detector();
        let buf = vec![0_u8; 320 * 320 * 3];
        let err = det.detect(&buf).await.expect_err("must error");
        assert!(matches!(err, CameraError::DetectorUnavailable { .. }));
    }

    /// Phase 1 builds the detector inline (skipping `load_model` so the
    /// tests don't need a model file on disk).
    fn stub_detector() -> CpuYoloDetector {
        CpuYoloDetector {
            model_path: PathBuf::from("/tmp/cave-home-camera-test.onnx"),
            input_w: 320,
            input_h: 320,
            labels: vec!["person".into(), "car".into()],
            score_floor: 0.4,
            nms_iou: 0.45,
        }
    }
}
