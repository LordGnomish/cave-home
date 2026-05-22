// SPDX-License-Identifier: Apache-2.0
//! NVIDIA TensorRT detector.
//!
//! Upstream: blakeblackshear/frigate@416a9b7692e052be98ad503704d26c7ef7a4c88d
//! :: frigate/detectors/plugins/tensorrt.py :: `TensorRtDetector`.
//! Frigate uses the PyCUDA + tensorrt python bindings; the user-space
//! deps are `libcuda.so.1` + `libnvinfer.so.X` shipped with the NVIDIA
//! Container Toolkit / driver.
//!
//! Linux-only and gated on the `nvidia-trt` feature for the same reason
//! Coral is — TensorRT does not ship a macOS / Windows distribution
//! suitable for the Frigate pipeline.

use std::path::PathBuf;

use async_trait::async_trait;

use crate::detectors::{Detection, Detector};
use crate::error::{CameraError, CameraResult};

/// Detector backed by TensorRT engine files.
#[derive(Debug)]
pub struct NvidiaDetector {
    // Reserved for Linux-side FFI (TensorRT + libcuda); held so the
    // engine pin survives a hot-swap.
    #[allow(dead_code)]
    engine_path: PathBuf,
    #[allow(dead_code)]
    labels: Vec<String>,
    input_w: u32,
    input_h: u32,
    #[allow(dead_code)]
    score_floor: f32,
}

impl NvidiaDetector {
    /// Default NVIDIA model input geometry (Frigate's bundled YOLOv7
    /// engine is 416×416).
    pub const DEFAULT_INPUT: (u32, u32) = (416, 416);

    /// Construct + load.
    pub fn load_model(
        engine_path: impl Into<PathBuf>,
        labels: Vec<String>,
    ) -> CameraResult<Self> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (engine_path, labels);
            Err(CameraError::DetectorUnavailable {
                reason: "NVIDIA TensorRT requires libcuda/libnvinfer (Linux only)".into(),
            })
        }
        #[cfg(target_os = "linux")]
        {
            let engine_path = engine_path.into();
            if !engine_path.exists() {
                return Err(CameraError::DetectorLoad(format!(
                    "TensorRT engine not found at {}",
                    engine_path.display()
                )));
            }
            Ok(Self {
                engine_path,
                labels,
                input_w: Self::DEFAULT_INPUT.0,
                input_h: Self::DEFAULT_INPUT.1,
                score_floor: 0.4,
            })
        }
    }
}

#[async_trait]
impl Detector for NvidiaDetector {
    fn input_shape(&self) -> (u32, u32) {
        (self.input_w, self.input_h)
    }

    async fn detect(&self, tensor_input: &[u8]) -> CameraResult<Vec<Detection>> {
        let expected = self.input_w as usize * self.input_h as usize * 3;
        if tensor_input.len() != expected {
            return Err(CameraError::DetectorInference(format!(
                "tensor input size {} != expected {expected}",
                tensor_input.len()
            )));
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(CameraError::DetectorUnavailable {
                reason: "NVIDIA TensorRT requires Linux".into(),
            })
        }
        #[cfg(target_os = "linux")]
        {
            let _ = (&self.engine_path, &self.labels, self.score_floor);
            Err(CameraError::DetectorUnavailable {
                reason: "NVIDIA TensorRT FFI not wired in this build (Phase 1b)".into(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(target_os = "linux"))]
    fn load_model_unavailable_on_non_linux() {
        let err = NvidiaDetector::load_model("/x", vec!["person".into()])
            .expect_err("must be unavailable");
        assert!(matches!(err, CameraError::DetectorUnavailable { .. }));
    }

    #[test]
    fn input_shape_is_416_x_416() {
        let det = NvidiaDetector {
            engine_path: PathBuf::from("/tmp/x.engine"),
            labels: vec!["person".into()],
            input_w: 416,
            input_h: 416,
            score_floor: 0.4,
        };
        assert_eq!(det.input_shape(), (416, 416));
    }
}
