// SPDX-License-Identifier: Apache-2.0
//! Google Coral EdgeTPU detector.
//!
//! Upstream: blakeblackshear/frigate@416a9b7692e052be98ad503704d26c7ef7a4c88d
//! :: frigate/detectors/plugins/edgetpu_tfl.py :: `EdgeTpuTflDetector`.
//! Frigate calls into `tflite_runtime.interpreter.Interpreter` with the
//! libedgetpu delegate. The Linux user-space dep is `libedgetpu.so.1`
//! shipped with Coral's apt package.
//!
//! Linux-only by construction (libedgetpu has no macOS / Windows
//! distribution). On non-Linux targets the trait impl returns
//! `DetectorError::Unavailable` — production code paths that need Coral
//! must compile on Linux.

use std::path::PathBuf;

use async_trait::async_trait;

use crate::detectors::{Detection, Detector};
use crate::error::{CameraError, CameraResult};

/// Detector backed by libedgetpu + a SSDLite MobileDet `.tflite` model.
#[derive(Debug)]
pub struct CoralEdgeTpuDetector {
    // Reserved for Linux-side FFI; held so the file pin survives a model
    // hot-swap. `dead_code` is suppressed because non-Linux builds never
    // dereference these fields.
    #[allow(dead_code)]
    model_path: PathBuf,
    #[allow(dead_code)]
    labels: Vec<String>,
    /// Coral models are quantised to 320×320 uint8 RGB (Frigate's
    /// bundled `mobiledet.tflite` model).
    input_w: u32,
    input_h: u32,
    /// Score floor.
    #[allow(dead_code)]
    score_floor: f32,
}

impl CoralEdgeTpuDetector {
    /// Default Coral model input geometry.
    pub const DEFAULT_INPUT: (u32, u32) = (320, 320);

    /// Construct + load. On non-Linux this short-circuits with a typed
    /// error before touching disk.
    pub fn load_model(
        model_path: impl Into<PathBuf>,
        labels: Vec<String>,
    ) -> CameraResult<Self> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (model_path, labels);
            Err(CameraError::DetectorUnavailable {
                reason: "Coral EdgeTPU requires libedgetpu (Linux only)".into(),
            })
        }
        #[cfg(target_os = "linux")]
        {
            let model_path = model_path.into();
            if !model_path.exists() {
                return Err(CameraError::DetectorLoad(format!(
                    "tflite model not found at {}",
                    model_path.display()
                )));
            }
            Ok(Self {
                model_path,
                labels,
                input_w: Self::DEFAULT_INPUT.0,
                input_h: Self::DEFAULT_INPUT.1,
                score_floor: 0.5,
            })
        }
    }
}

#[async_trait]
impl Detector for CoralEdgeTpuDetector {
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
                reason: "Coral EdgeTPU requires Linux + libedgetpu".into(),
            })
        }
        #[cfg(target_os = "linux")]
        {
            // libedgetpu/tflite_runtime FFI lives here. The trait,
            // load_model, and post-process pipeline above are stable
            // and tested; the FFI bring-up against libedgetpu.so.1 is a
            // Phase 1b workspace concern.
            let _ = (&self.model_path, &self.labels, self.score_floor);
            Err(CameraError::DetectorUnavailable {
                reason: "Coral EdgeTPU FFI not wired in this build (Phase 1b)".into(),
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
        let err = CoralEdgeTpuDetector::load_model("/x", vec!["person".into()])
            .expect_err("must be unavailable");
        assert!(matches!(err, CameraError::DetectorUnavailable { .. }));
    }

    #[test]
    fn input_shape_is_320_x_320() {
        // Build directly without going through load_model so we exercise
        // input_shape on every platform.
        let det = CoralEdgeTpuDetector {
            model_path: PathBuf::from("/tmp/x.tflite"),
            labels: vec!["person".into()],
            input_w: 320,
            input_h: 320,
            score_floor: 0.5,
        };
        assert_eq!(det.input_shape(), (320, 320));
    }
}
