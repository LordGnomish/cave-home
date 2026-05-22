// SPDX-License-Identifier: Apache-2.0
//! Single-import convenience for downstream crates.

pub use crate::birdseye::{BirdseyeLayout, MosaicCell, compose_y_plane, plan_layout};
pub use crate::capture::{FfmpegCommand, FrameSource, RtspCapture, YuvFrame, build_capture_argv};
pub use crate::config::{CameraConfig, DetectorKind, MotionConfig, RecordConfig};
pub use crate::detectors::{
    CoralEdgeTpuDetector, CpuYoloDetector, Detection, Detector, MockDetector, NvidiaDetector,
};
pub use crate::error::{CameraError, CameraResult};
pub use crate::events::{CameraEvent, CameraEventSink, EventKind, NullEventSink, RecordingEventSink};
pub use crate::motion::{MotionDetector, MotionResult};
pub use crate::record::{EventClip, EventClipWriter, Segment, SegmentLog};
pub use crate::tracker::{IouTracker, TrackTransition, TrackedObject};
