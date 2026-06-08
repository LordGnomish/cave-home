//! The recording-mode decision.
//!
//! A UniFi Protect camera is configured with a recording mode; the live answer
//! the household (and the storage layer) actually needs is "should this camera
//! be writing video *right now*?". That depends on the mode, on whether there
//! is a live detection that an armed zone cares about, and — for the Schedule
//! mode — on whether the current moment falls inside the camera's recording
//! schedule. This module computes exactly that, as pure logic.
//!
//! Modelled from the public Protect recording-mode set and the HA
//! `unifiprotect` integration (Apache-2.0). No GPL source was read.

use crate::privacy::TimeOfDay;

/// The recording mode a camera is operating in.
///
/// Distinct from [`crate::device::RecordingMode`] only in that this is the enum
/// the decision function consumes; they carry the same four cases and convert
/// freely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordMode {
    /// Never record, no matter what.
    Never,
    /// Always record (continuous).
    Always,
    /// Record whenever an armed detection is live.
    Detections,
    /// Record only inside the recording schedule.
    Schedule,
}

impl From<crate::device::RecordingMode> for RecordMode {
    fn from(m: crate::device::RecordingMode) -> Self {
        match m {
            crate::device::RecordingMode::Never => Self::Never,
            crate::device::RecordingMode::Always => Self::Always,
            crate::device::RecordingMode::Detections => Self::Detections,
            crate::device::RecordingMode::Schedule => Self::Schedule,
        }
    }
}

/// A daily recording schedule: a single inclusive window `[start, end]`.
///
/// A window that wraps past midnight (e.g. 22:00 → 06:00) is supported —
/// `start` later than `end` is read as "overnight".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    start: TimeOfDay,
    end: TimeOfDay,
}

impl Schedule {
    /// A schedule active from `start` to `end` (inclusive). If `start > end`
    /// the window is treated as wrapping over midnight.
    #[must_use]
    pub const fn new(start: TimeOfDay, end: TimeOfDay) -> Self {
        Self { start, end }
    }

    /// Whether `now` falls inside the schedule window.
    #[must_use]
    pub fn contains(&self, now: TimeOfDay) -> bool {
        let (s, e, n) = (
            self.start.minutes(),
            self.end.minutes(),
            now.minutes(),
        );
        if s <= e {
            // Same-day window, inclusive on both ends.
            n >= s && n <= e
        } else {
            // Overnight window: active late tonight OR early tomorrow.
            n >= s || n <= e
        }
    }
}

/// Should the camera be recording right now?
///
/// - [`RecordMode::Never`] → never.
/// - [`RecordMode::Always`] → always.
/// - [`RecordMode::Detections`] → only while an armed detection is live
///   (`detection_active`).
/// - [`RecordMode::Schedule`] → only while inside the schedule window
///   (`in_schedule`).
///
/// This is the mode-level decision; for the Schedule case the caller passes the
/// already-evaluated [`Schedule::contains`] result as `in_schedule`. See
/// [`should_record_scheduled`] for the convenience that takes the schedule and
/// the clock directly.
#[must_use]
pub fn should_record(mode: RecordMode, detection_active: bool, in_schedule: bool) -> bool {
    match mode {
        RecordMode::Never => false,
        RecordMode::Always => true,
        RecordMode::Detections => detection_active,
        RecordMode::Schedule => in_schedule,
    }
}

/// The Schedule-mode decision with the schedule and clock supplied directly.
#[must_use]
pub fn should_record_scheduled(schedule: Schedule, now: TimeOfDay) -> bool {
    should_record(RecordMode::Schedule, false, schedule.contains(now))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(h: u8, m: u8) -> TimeOfDay {
        TimeOfDay::new(h, m).expect("valid time")
    }

    #[test]
    fn never_mode_never_records() {
        assert!(!should_record(RecordMode::Never, true, true));
        assert!(!should_record(RecordMode::Never, false, false));
    }

    #[test]
    fn always_mode_always_records() {
        assert!(should_record(RecordMode::Always, false, false));
        assert!(should_record(RecordMode::Always, true, true));
    }

    #[test]
    fn detections_mode_follows_the_detection() {
        assert!(should_record(RecordMode::Detections, true, false));
        assert!(!should_record(RecordMode::Detections, false, true));
    }

    #[test]
    fn schedule_mode_follows_the_schedule_flag() {
        assert!(should_record(RecordMode::Schedule, false, true));
        assert!(!should_record(RecordMode::Schedule, true, false));
    }

    #[test]
    fn same_day_schedule_is_inclusive() {
        let s = Schedule::new(t(9, 0), t(17, 0));
        assert!(s.contains(t(9, 0)));
        assert!(s.contains(t(12, 30)));
        assert!(s.contains(t(17, 0)));
        assert!(!s.contains(t(8, 59)));
        assert!(!s.contains(t(17, 1)));
    }

    #[test]
    fn overnight_schedule_wraps_midnight() {
        let s = Schedule::new(t(22, 0), t(6, 0));
        assert!(s.contains(t(23, 0)));
        assert!(s.contains(t(0, 0)));
        assert!(s.contains(t(5, 59)));
        assert!(s.contains(t(6, 0)));
        assert!(!s.contains(t(12, 0)));
        assert!(!s.contains(t(21, 59)));
    }

    #[test]
    fn scheduled_helper_matches_window() {
        let s = Schedule::new(t(8, 0), t(20, 0));
        assert!(should_record_scheduled(s, t(10, 0)));
        assert!(!should_record_scheduled(s, t(21, 0)));
    }

    #[test]
    fn device_recording_mode_converts() {
        assert_eq!(
            RecordMode::from(crate::device::RecordingMode::Always),
            RecordMode::Always
        );
        assert_eq!(
            RecordMode::from(crate::device::RecordingMode::Detections),
            RecordMode::Detections
        );
    }
}
