// SPDX-License-Identifier: Apache-2.0
//! Camera configuration types.
//!
//! Upstream: blakeblackshear/frigate@416a9b7692e052be98ad503704d26c7ef7a4c88d
//! :: frigate/config/config.py :: `FrigateConfig`, `CameraConfig`,
//! `MotionConfig`, `RecordConfig`, `DetectConfig`.
//!
//! Frigate's config object is huge (>1k lines of pydantic models).
//! Phase 1 ports just the fields the runtime pipeline actually reads.
//! Everything else lives in `parity.manifest.toml` `[[unmapped]]`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{CameraError, CameraResult};

/// One camera.
///
/// Port of `frigate.config.config.CameraConfig` (cut down to the Phase 1
/// fields). Grandma-friendly: this is what backs a single tile in the
/// `/admin/camera` grid.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CameraConfig {
    /// Unique camera key (also displayed as "Kamera adı" in the Portal).
    pub name: String,

    /// RTSP / file source URL. Passed verbatim to `ffmpeg -i ...`.
    pub source: String,

    /// Stream geometry — width pixels (`detect.width` in Frigate YAML).
    pub width: u32,

    /// Stream geometry — height pixels (`detect.height` in Frigate YAML).
    pub height: u32,

    /// Target frame rate the capture loop will produce, after downsampling
    /// from the RTSP source. Frigate clamps detect.fps to ≤ source fps.
    pub fps: u32,

    /// Detector to use for this camera.
    pub detector: DetectorKind,

    /// Motion-detection parameters.
    #[serde(default)]
    pub motion: MotionConfig,

    /// Recording parameters.
    #[serde(default)]
    pub record: RecordConfig,
}

impl CameraConfig {
    /// Bytes per raw YUV420p frame for the configured geometry.
    /// Frigate writes raw frames in YUV420p (`-f rawvideo -pix_fmt yuv420p`),
    /// which is 1.5 bytes per pixel.
    #[must_use]
    pub fn frame_bytes(&self) -> usize {
        let pixels = self.width as usize * self.height as usize;
        pixels + pixels / 2
    }

    /// Validate the config (mirrors `frigate.config.config.runtime_config`).
    pub fn validate(&self) -> CameraResult<()> {
        if self.name.is_empty() {
            return Err(CameraError::Config("camera name is empty".into()));
        }
        if self.source.is_empty() {
            return Err(CameraError::Config(format!(
                "camera {name:?} has empty source",
                name = self.name
            )));
        }
        if self.width == 0 || self.height == 0 {
            return Err(CameraError::Config(format!(
                "camera {name:?} has zero width/height",
                name = self.name
            )));
        }
        if self.fps == 0 {
            return Err(CameraError::Config(format!(
                "camera {name:?} has fps=0",
                name = self.name
            )));
        }
        Ok(())
    }
}

/// Which detector implementation this camera uses.
///
/// Port of `frigate.detectors.DetectorTypeEnum`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectorKind {
    /// CPU YOLOv8/YOLOv9 via onnxruntime.
    #[default]
    CpuYolo,
    /// Google Coral EdgeTPU (Linux only).
    CoralEdgeTpu,
    /// NVIDIA TensorRT (Linux only).
    NvidiaTrt,
    /// In-process deterministic mock — production code never picks this.
    /// Tests pick it explicitly.
    Mock,
}

/// Motion-detection knobs. Port of `frigate.config.config.MotionConfig`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MotionConfig {
    /// Per-pixel luminance delta above which a pixel is "moving"
    /// (Frigate default: 30).
    pub threshold: u8,

    /// Minimum contiguous-moving-pixel count required to fire a motion
    /// event (Frigate default: 30).
    pub contour_area: u32,

    /// Exponential running-average weight on the background model
    /// (Frigate default: 0.05 — slow adaptation so a parked car isn't
    /// absorbed in five frames).
    pub frame_alpha: f32,
}

impl Default for MotionConfig {
    fn default() -> Self {
        Self {
            threshold: 30,
            contour_area: 30,
            frame_alpha: 0.05,
        }
    }
}

/// Recording knobs. Port of `frigate.config.config.RecordConfig`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RecordConfig {
    /// Master switch.
    pub enabled: bool,

    /// Where MP4 segments are written.
    pub root: PathBuf,

    /// Segment length, seconds (Frigate default: 10).
    pub segment_seconds: u32,

    /// Keep events younger than this many days.
    pub event_retention_days: u32,
}

impl Default for RecordConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            root: PathBuf::from("/var/lib/cave-home/camera"),
            segment_seconds: 10,
            event_retention_days: 14,
        }
    }
}
