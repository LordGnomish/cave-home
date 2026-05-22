// SPDX-License-Identifier: Apache-2.0
//! MP4 segment bookkeeping.
//!
//! Upstream: blakeblackshear/frigate@416a9b7692e052be98ad503704d26c7ef7a4c88d
//! :: frigate/record/maintainer.py :: `SegmentMaintainer.move_segment`
//! and frigate/record/cleanup.py :: `RecordingCleanup.expire_recordings`.
//!
//! Frigate spawns a second ffmpeg per camera with a `segment` muxer
//! that writes 10-second MP4 chunks to disk; a Python loop renames /
//! catalogues them. Phase 1 ports the **catalogue / retention**
//! half of that loop (the data-plane segment muxer is a Phase 1b
//! ffmpeg-argv concern). The catalogue is what powers the
//! `/admin/camera/:id/events` timeline.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// One MP4 segment recorded on disk.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Segment {
    /// Camera key the segment belongs to.
    pub camera: String,
    /// Absolute path on disk.
    pub path: PathBuf,
    /// Start time (unix millis).
    pub start_ms: u128,
    /// Duration (seconds, matches `RecordConfig.segment_seconds`).
    pub duration_s: u32,
}

/// In-memory segment catalogue (per process).
#[derive(Debug, Default)]
pub struct SegmentLog {
    /// Index: (camera, start_ms) -> segment.
    by_camera: Mutex<BTreeMap<(String, u128), Segment>>,
}

impl SegmentLog {
    /// New empty log.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a freshly-closed segment.
    pub fn record(&self, seg: Segment) {
        let mut g = self.by_camera.lock();
        g.insert((seg.camera.clone(), seg.start_ms), seg);
    }

    /// All segments for a camera, sorted by start time.
    #[must_use]
    pub fn segments(&self, camera: &str) -> Vec<Segment> {
        let g = self.by_camera.lock();
        g.iter()
            .filter(|((c, _), _)| c == camera)
            .map(|(_, s)| s.clone())
            .collect()
    }

    /// Number of segments tracked across all cameras.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_camera.lock().len()
    }

    /// Whether the log is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_camera.lock().is_empty()
    }

    /// Expire (drop from the catalogue) segments older than `retention_days`
    /// when measured against `now`. Returns the dropped segments so the
    /// caller can `fs::remove_file` them.
    pub fn expire(&self, now: SystemTime, retention_days: u32) -> Vec<Segment> {
        let cutoff_ms = match now.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(d) => d
                .as_millis()
                .saturating_sub(u128::from(retention_days) * 24 * 3600 * 1000),
            Err(_) => return Vec::new(),
        };
        let mut g = self.by_camera.lock();
        let to_drop: Vec<(String, u128)> = g
            .iter()
            .filter(|((_, start), _)| *start < cutoff_ms)
            .map(|(k, _)| k.clone())
            .collect();
        let mut dropped = Vec::with_capacity(to_drop.len());
        for k in to_drop {
            if let Some(s) = g.remove(&k) {
                dropped.push(s);
            }
        }
        dropped
    }
}

/// Convenience: derive the canonical on-disk path Frigate uses:
/// `<root>/<camera>/<yyyy-mm-dd>/<hh.mm.ss>.mp4`.
///
/// Port of `frigate.record.maintainer.SegmentMaintainer.move_segment`
/// without the actual rename (we only build the path; the mux step
/// performs the move during Phase 1b).
#[must_use]
pub fn canonical_segment_path(root: &Path, camera: &str, start_ms: u128) -> PathBuf {
    let secs = (start_ms / 1000) as i64;
    let (date, time) = format_ymd_hms(secs);
    root.join(camera).join(date).join(format!("{time}.mp4"))
}

/// UTC `(yyyy-mm-dd, hh.mm.ss)` from a Unix timestamp in seconds.
/// Pure, no chrono dep.
fn format_ymd_hms(secs: i64) -> (String, String) {
    // Simple proleptic Gregorian computation. Avoids pulling in chrono
    // for a single date stamp. Days since 1970-01-01.
    let secs_per_day: i64 = 86_400;
    let mut days = secs.div_euclid(secs_per_day);
    let rem = secs.rem_euclid(secs_per_day);
    let hh = rem / 3600;
    let mm = (rem % 3600) / 60;
    let ss = rem % 60;

    let mut year = 1970_i64;
    loop {
        let ydays = if is_leap_year(year) { 366 } else { 365 };
        if days < ydays {
            break;
        }
        days -= ydays;
        year += 1;
    }
    let month_lens = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 1_i64;
    for m in month_lens {
        if days < m {
            break;
        }
        days -= m;
        month += 1;
    }
    let day = days + 1;
    (
        format!("{year:04}-{month:02}-{day:02}"),
        format!("{hh:02}.{mm:02}.{ss:02}"),
    )
}

const fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn seg(camera: &str, start_ms: u128) -> Segment {
        Segment {
            camera: camera.into(),
            path: PathBuf::from(format!("/tmp/{camera}/{start_ms}.mp4")),
            start_ms,
            duration_s: 10,
        }
    }

    #[test]
    fn record_and_query_segments_for_one_camera() {
        let log = SegmentLog::new();
        log.record(seg("front", 1_000));
        log.record(seg("front", 2_000));
        log.record(seg("back", 1_000));
        let front = log.segments("front");
        assert_eq!(front.len(), 2);
        let back = log.segments("back");
        assert_eq!(back.len(), 1);
        assert_eq!(log.len(), 3);
    }

    #[test]
    fn segments_are_sorted_by_start_ms() {
        let log = SegmentLog::new();
        log.record(seg("front", 3_000));
        log.record(seg("front", 1_000));
        log.record(seg("front", 2_000));
        let s = log.segments("front");
        assert_eq!(s[0].start_ms, 1_000);
        assert_eq!(s[1].start_ms, 2_000);
        assert_eq!(s[2].start_ms, 3_000);
    }

    #[test]
    fn expire_drops_old_segments_only() {
        let log = SegmentLog::new();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(86_400 * 30);
        // 30 days old segment -> should be dropped (retention 14).
        log.record(seg("front", 0));
        // 1-day-old segment -> should be kept.
        log.record(seg(
            "front",
            u128::from(29_u32) * 86_400 * 1_000,
        ));
        let dropped = log.expire(now, 14);
        assert_eq!(dropped.len(), 1);
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn canonical_segment_path_layout_matches_frigate() {
        let p = canonical_segment_path(
            Path::new("/var/lib/cave-home/camera"),
            "front",
            1_700_000_000_000,
        );
        // Stripped form: front/<date>/<time>.mp4. We only assert the
        // structure (date/time stamp varies but is deterministic).
        let s = p.to_string_lossy().to_string();
        assert!(s.starts_with("/var/lib/cave-home/camera/front/"));
        assert!(s.ends_with(".mp4"));
        // Date directory matches yyyy-mm-dd.
        let parts: Vec<&str> = s.split('/').collect();
        let date = parts.iter().rev().nth(1).expect("date dir");
        let pieces: Vec<&str> = date.split('-').collect();
        assert_eq!(pieces.len(), 3);
        assert_eq!(pieces[0].len(), 4);
        assert_eq!(pieces[1].len(), 2);
        assert_eq!(pieces[2].len(), 2);
    }

    #[test]
    fn date_formatter_2024_03_01_is_leap_year_aware() {
        // 2024-03-01 00:00:00 UTC = 1709251200 unix seconds.
        let (d, t) = format_ymd_hms(1_709_251_200);
        assert_eq!(d, "2024-03-01");
        assert_eq!(t, "00.00.00");
    }
}
