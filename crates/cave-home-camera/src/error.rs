// SPDX-License-Identifier: Apache-2.0
//! Top-level error union for the camera crate.
//!
//! Upstream: blakeblackshear/frigate@416a9b7692e052be98ad503704d26c7ef7a4c88d
//! :: frigate/util/builtin.py + scattered `raise` sites across
//! `video.py`, `motion/`, `detectors/`, `record/`. Frigate uses bare Python
//! exceptions; we widen them into one typed enum.

use std::io;
use std::path::PathBuf;
use thiserror::Error;

/// Crate-wide `Result` alias.
pub type CameraResult<T> = Result<T, CameraError>;

/// Top-level error union covering capture, decode, motion, detect, track,
/// record, mux paths.
#[derive(Debug, Error)]
pub enum CameraError {
    /// Configuration failed validation.
    #[error("invalid configuration: {0}")]
    Config(String),

    /// `ffmpeg` sub-process could not be spawned or exited unexpectedly.
    #[error("ffmpeg sub-process failure: {0}")]
    Ffmpeg(String),

    /// RTSP source returned no frames within the read deadline.
    #[error("capture timeout reading from {url}")]
    CaptureTimeout {
        /// RTSP/file source URL.
        url: String,
    },

    /// Frame buffer was the wrong size for the configured stream geometry.
    #[error(
        "frame size mismatch: got {got} bytes, expected {expected} bytes ({width}x{height} yuv420p)"
    )]
    FrameSize {
        /// Bytes actually read.
        got: usize,
        /// Bytes the YUV420p layout demands.
        expected: usize,
        /// Configured width.
        width: u32,
        /// Configured height.
        height: u32,
    },

    /// Detector failed to load its model file.
    #[error("detector load failed: {0}")]
    DetectorLoad(String),

    /// Detector ran but returned an internal error (NaN, shape mismatch, ...).
    #[error("detector inference failed: {0}")]
    DetectorInference(String),

    /// Detector implementation is not available on this build (e.g. Coral on
    /// macOS, Nvidia without TensorRT, `cpu-yolo` feature off).
    #[error("detector unavailable on this platform / build: {reason}")]
    DetectorUnavailable {
        /// Human-readable reason.
        reason: String,
    },

    /// File-system IO error (recording / model / event paths).
    #[error("io error on {path:?}: {source}")]
    Io {
        /// Path that triggered the error (may be empty if not path-scoped).
        path: PathBuf,
        /// Underlying IO error.
        #[source]
        source: io::Error,
    },

    /// Recording segment writer reached an inconsistent state.
    #[error("recording error: {0}")]
    Recording(String),

    /// Event sink delivery failed.
    #[error("event sink delivery failed: {0}")]
    EventSink(String),
}

impl CameraError {
    /// Wrap a `std::io::Error` against a path.
    pub fn io<P: Into<PathBuf>>(path: P, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
