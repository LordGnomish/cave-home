// SPDX-License-Identifier: Apache-2.0
//! RTSP / file capture via `ffmpeg` sub-process.
//!
//! Upstream: blakeblackshear/frigate@416a9b7692e052be98ad503704d26c7ef7a4c88d
//! :: frigate/video.py + frigate/ffmpeg_presets.py.
//!
//! Frigate spawns one `ffmpeg` process per camera, streams raw YUV420p
//! frames out of its stdout, and parses them in a Python thread. We do
//! the same with `tokio::process::Command`.

pub mod ffmpeg;
pub mod rtsp;

pub use ffmpeg::{FfmpegCommand, build_capture_argv};
pub use rtsp::{FrameSource, RtspCapture, YuvFrame};
