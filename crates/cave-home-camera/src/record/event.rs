// SPDX-License-Identifier: Apache-2.0
//! Event clip writer — slice contiguous segments around a detection
//! event into a single-file MP4 clip.
//!
//! Upstream: blakeblackshear/frigate@416a9b7692e052be98ad503704d26c7ef7a4c88d
//! :: frigate/record/maintainer.py :: `SegmentMaintainer.write_event` +
//! `EventProcessor.write_clip`.
//!
//! Frigate runs ffmpeg `-ss <start> -t <duration> -c copy` against the
//! catalogued segment files to cut a clip without re-encode. Phase 1
//! ports the **clip-selection** half of that pipeline (decide which
//! segments cover the event window). The actual ffmpeg invocation is a
//! Phase 1b workspace concern.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::record::segment::{Segment, SegmentLog};

/// One event clip request.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct EventClip {
    /// Camera key.
    pub camera: String,
    /// Event start (unix millis).
    pub start_ms: u128,
    /// Event end (unix millis).
    pub end_ms: u128,
    /// Where the resulting clip should be written.
    pub output_path: PathBuf,
    /// Segments that cover the requested window, ordered by start_ms.
    pub source_segments: Vec<Segment>,
}

/// Pure helper: pick the segments from `log` that cover the
/// `[start_ms, end_ms]` window.
#[derive(Debug, Default)]
pub struct EventClipWriter;

impl EventClipWriter {
    /// New writer.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Decide which catalogued segments cover the event window. A segment
    /// covers the window iff its `[start, start+duration)` interval
    /// overlaps `[start_ms, end_ms]`.
    pub fn plan(
        &self,
        log: &SegmentLog,
        camera: &str,
        start_ms: u128,
        end_ms: u128,
        output_path: PathBuf,
    ) -> EventClip {
        let segs = log.segments(camera);
        let covers: Vec<Segment> = segs
            .into_iter()
            .filter(|s| {
                let s_end = s.start_ms + u128::from(s.duration_s) * 1000;
                s_end > start_ms && s.start_ms < end_ms
            })
            .collect();
        EventClip {
            camera: camera.into(),
            start_ms,
            end_ms,
            output_path,
            source_segments: covers,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(camera: &str, start_ms: u128) -> Segment {
        Segment {
            camera: camera.into(),
            path: PathBuf::from(format!("/tmp/{camera}/{start_ms}.mp4")),
            start_ms,
            duration_s: 10,
        }
    }

    #[test]
    fn plan_picks_only_overlapping_segments() {
        let log = SegmentLog::new();
        // Segments at: 0, 10s, 20s, 30s (each 10s wide).
        log.record(seg("front", 0));
        log.record(seg("front", 10_000));
        log.record(seg("front", 20_000));
        log.record(seg("front", 30_000));
        // Event window: 15s..25s. Should cover the 10s and 20s segs only.
        let clip = EventClipWriter::new().plan(
            &log,
            "front",
            15_000,
            25_000,
            PathBuf::from("/tmp/clip.mp4"),
        );
        assert_eq!(clip.source_segments.len(), 2);
        let starts: Vec<u128> = clip.source_segments.iter().map(|s| s.start_ms).collect();
        assert_eq!(starts, vec![10_000, 20_000]);
    }

    #[test]
    fn plan_with_no_overlap_yields_empty_clip() {
        let log = SegmentLog::new();
        log.record(seg("front", 0));
        let clip = EventClipWriter::new().plan(
            &log,
            "front",
            100_000,
            200_000,
            PathBuf::from("/tmp/clip.mp4"),
        );
        assert!(clip.source_segments.is_empty());
    }

    #[test]
    fn plan_filters_by_camera() {
        let log = SegmentLog::new();
        log.record(seg("front", 0));
        log.record(seg("back", 0));
        let clip = EventClipWriter::new().plan(
            &log,
            "front",
            0,
            5_000,
            PathBuf::from("/tmp/clip.mp4"),
        );
        assert_eq!(clip.source_segments.len(), 1);
        assert_eq!(clip.source_segments[0].camera, "front");
    }
}
