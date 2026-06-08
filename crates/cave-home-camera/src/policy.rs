//! When to start a clip, when to stop it, when to fire an alert, and how long
//! to keep what was saved.
//!
//! This is the camera brain's decision layer. It sits between "the tracker says
//! a person is in the driveway" and "write an MP4 / send a notification" — the
//! parts that are deferred to Phase 1b. None of these decisions need a video
//! frame; they are pure functions of detection events and caller-supplied time,
//! which is what makes them testable here in full.
//!
//! Three concerns live together:
//! - [`ClipPolicy`] decides when an event recording starts and stops, padding
//!   the real activity with a pre-roll and a post-roll so the saved clip shows
//!   the lead-up and the aftermath, not just the middle.
//! - [`Debounce`] stops a flickery detector firing twenty "person" alerts for
//!   one visitor — within `quiet` seconds of the last alert for a label, a new
//!   one is suppressed.
//! - [`retention`] decides whether a saved event is old enough to delete, given
//!   the per-label retention the household configured and the current time the
//!   caller supplies.

use crate::detection::Tick;
use crate::label::ObjectLabel;

/// What the recording layer should do this tick. The brain emits these; the
/// (Phase-1b) recorder acts on them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipAction {
    /// Nothing to do — no activity, no recording in progress.
    Idle,
    /// Begin an event recording. The carried tick is where the saved clip
    /// should start, i.e. `now - pre_roll` (saturated at zero).
    Start(Tick),
    /// An event recording is already running and activity continues.
    Continue,
    /// Stop the recording. The carried tick is where the saved clip should end,
    /// i.e. `last_activity + post_roll`.
    Stop(Tick),
}

/// Decides when an event clip starts and stops, with pre/post-roll padding.
///
/// Drive it one tick at a time with [`ClipPolicy::observe`], passing whether any
/// relevant detection is active this tick. The policy holds only "am I
/// recording, and when did I last see activity" — it reads no clock.
#[derive(Debug, Clone)]
pub struct ClipPolicy {
    pre_roll: Tick,
    post_roll: Tick,
    recording: bool,
    last_activity: Option<Tick>,
}

impl ClipPolicy {
    /// A policy that pads the saved clip by `pre_roll` seconds before the first
    /// activity and keeps recording until `post_roll` seconds after the last
    /// activity.
    #[must_use]
    pub const fn new(pre_roll: Tick, post_roll: Tick) -> Self {
        Self {
            pre_roll,
            post_roll,
            recording: false,
            last_activity: None,
        }
    }

    /// Whether a recording is currently in progress.
    #[must_use]
    pub const fn is_recording(&self) -> bool {
        self.recording
    }

    /// Advance the policy by one tick. `active` is whether any detection the
    /// camera cares about is present this tick; `now` is the current tick.
    ///
    /// - Idle + active → [`ClipAction::Start`] at `now - pre_roll`.
    /// - Recording + active → [`ClipAction::Continue`] (the post-roll clock is
    ///   reset to `now`).
    /// - Recording + idle, still within the post-roll window →
    ///   [`ClipAction::Continue`].
    /// - Recording + idle, post-roll elapsed → [`ClipAction::Stop`] at
    ///   `last_activity + post_roll`.
    /// - Idle + idle → [`ClipAction::Idle`].
    pub fn observe(&mut self, active: bool, now: Tick) -> ClipAction {
        if active {
            self.last_activity = Some(now);
            if self.recording {
                ClipAction::Continue
            } else {
                self.recording = true;
                ClipAction::Start(now.saturating_sub(self.pre_roll))
            }
        } else if self.recording {
            // No activity this tick; are we still inside the post-roll tail?
            let last = self.last_activity.unwrap_or(now);
            let stop_at = last.saturating_add(self.post_roll);
            if now >= stop_at {
                self.recording = false;
                ClipAction::Stop(stop_at)
            } else {
                ClipAction::Continue
            }
        } else {
            ClipAction::Idle
        }
    }
}

/// Per-label alert de-bounce: collapse a burst of detections of the same thing
/// into one alert.
///
/// A detector can re-fire "person" every frame for as long as someone is in
/// view. Without de-bounce that is a notification storm. [`Debounce::allow`]
/// accepts the first alert for a label, then suppresses further alerts for that
/// label until `quiet` seconds have elapsed. Each label has its own clock, so a
/// dog walking past does not silence the alert for a person arriving.
#[derive(Debug, Clone, Default)]
pub struct Debounce {
    last_fired: Vec<(ObjectLabel, Tick)>,
}

impl Debounce {
    /// A fresh de-bouncer that has fired nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            last_fired: Vec::new(),
        }
    }

    /// Decide whether an alert for `label` at `now` should fire, given a `quiet`
    /// window in seconds. Returns `true` and records the time if it fires;
    /// returns `false` if it is still within the quiet window of the last alert
    /// for this label. The boundary is inclusive: an alert exactly `quiet`
    /// seconds after the last is allowed. A `quiet` of zero fires every time.
    pub fn allow(&mut self, label: ObjectLabel, now: Tick, quiet: Tick) -> bool {
        for entry in &mut self.last_fired {
            if entry.0 == label {
                if now.saturating_sub(entry.1) >= quiet {
                    entry.1 = now;
                    return true;
                }
                return false;
            }
        }
        // First time we have seen this label.
        self.last_fired.push((label, now));
        true
    }
}

/// Whether a saved event of `label`, recorded at `recorded_at`, should be kept
/// or deleted as of `now`.
///
/// The household configures how many days events of each label live
/// (`retention_days`); the caller supplies `now` and the recording time in the
/// same tick unit (seconds). An event is kept while its age is at most the
/// retention window; once it is older it is classified [`Retention::Expired`]
/// and the (Phase-1b) storage layer may delete it. A retention of zero days
/// expires everything immediately; the age comparison is saturating so a
/// non-monotonic clock never wraps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retention {
    /// Still within its retention window.
    Keep,
    /// Older than its retention window — eligible for deletion.
    Expired,
}

/// Number of seconds in a day, for turning a retention-in-days into ticks.
pub const SECONDS_PER_DAY: Tick = 86_400;

/// Classify a saved event for retention. See [`Retention`].
#[must_use]
pub fn classify_retention(
    recorded_at: Tick,
    now: Tick,
    retention_days: u32,
) -> Retention {
    let window = Tick::from(retention_days).saturating_mul(SECONDS_PER_DAY);
    let age = now.saturating_sub(recorded_at);
    if age <= window {
        Retention::Keep
    } else {
        Retention::Expired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- ClipPolicy ----

    #[test]
    fn clip_starts_with_pre_roll_offset() {
        let mut p = ClipPolicy::new(5, 10);
        // Activity at tick 100 -> clip starts at 95.
        assert_eq!(p.observe(true, 100), ClipAction::Start(95));
        assert!(p.is_recording());
    }

    #[test]
    fn clip_start_pre_roll_saturates_at_zero() {
        let mut p = ClipPolicy::new(10, 5);
        assert_eq!(p.observe(true, 3), ClipAction::Start(0));
    }

    #[test]
    fn clip_continues_while_active() {
        let mut p = ClipPolicy::new(5, 10);
        p.observe(true, 100);
        assert_eq!(p.observe(true, 101), ClipAction::Continue);
        assert_eq!(p.observe(true, 102), ClipAction::Continue);
    }

    #[test]
    fn clip_holds_through_post_roll_then_stops() {
        let mut p = ClipPolicy::new(0, 10);
        p.observe(true, 100); // last activity 100
        // Idle but within post-roll (stop_at = 110).
        assert_eq!(p.observe(false, 105), ClipAction::Continue);
        assert_eq!(p.observe(false, 109), ClipAction::Continue);
        // At 110 the post-roll has elapsed -> stop, clip ends at 110.
        assert_eq!(p.observe(false, 110), ClipAction::Stop(110));
        assert!(!p.is_recording());
    }

    #[test]
    fn renewed_activity_resets_the_post_roll() {
        let mut p = ClipPolicy::new(0, 10);
        p.observe(true, 100);
        assert_eq!(p.observe(false, 105), ClipAction::Continue);
        // Activity comes back at 108 -> last_activity now 108, stop_at -> 118.
        assert_eq!(p.observe(true, 108), ClipAction::Continue);
        assert_eq!(p.observe(false, 117), ClipAction::Continue);
        assert_eq!(p.observe(false, 118), ClipAction::Stop(118));
    }

    #[test]
    fn idle_camera_emits_idle() {
        let mut p = ClipPolicy::new(5, 10);
        assert_eq!(p.observe(false, 0), ClipAction::Idle);
        assert!(!p.is_recording());
    }

    // ---- Debounce ----

    #[test]
    fn first_alert_for_a_label_fires() {
        let mut d = Debounce::new();
        assert!(d.allow(ObjectLabel::Person, 0, 30));
    }

    #[test]
    fn repeat_within_quiet_window_is_suppressed() {
        let mut d = Debounce::new();
        assert!(d.allow(ObjectLabel::Person, 0, 30));
        assert!(!d.allow(ObjectLabel::Person, 10, 30));
        assert!(!d.allow(ObjectLabel::Person, 29, 30));
    }

    #[test]
    fn alert_at_quiet_boundary_fires() {
        let mut d = Debounce::new();
        assert!(d.allow(ObjectLabel::Person, 0, 30));
        assert!(d.allow(ObjectLabel::Person, 30, 30));
    }

    #[test]
    fn each_label_has_its_own_quiet_window() {
        let mut d = Debounce::new();
        assert!(d.allow(ObjectLabel::Person, 0, 30));
        // A dog right after the person still fires — different label.
        assert!(d.allow(ObjectLabel::Dog, 1, 30));
        // But a second person is still suppressed.
        assert!(!d.allow(ObjectLabel::Person, 1, 30));
    }

    #[test]
    fn zero_quiet_fires_every_time() {
        let mut d = Debounce::new();
        assert!(d.allow(ObjectLabel::Person, 0, 0));
        assert!(d.allow(ObjectLabel::Person, 0, 0));
        assert!(d.allow(ObjectLabel::Person, 1, 0));
    }

    // ---- retention ----

    #[test]
    fn fresh_event_is_kept() {
        // Recorded at day 0, now half a day later, kept for 7 days.
        assert_eq!(
            classify_retention(0, SECONDS_PER_DAY / 2, 7),
            Retention::Keep
        );
    }

    #[test]
    fn event_exactly_at_window_edge_is_kept() {
        // Age exactly == window -> inclusive Keep.
        assert_eq!(
            classify_retention(0, 7 * SECONDS_PER_DAY, 7),
            Retention::Keep
        );
    }

    #[test]
    fn event_past_window_expires() {
        assert_eq!(
            classify_retention(0, 7 * SECONDS_PER_DAY + 1, 7),
            Retention::Expired
        );
    }

    #[test]
    fn zero_day_retention_expires_immediately() {
        assert_eq!(classify_retention(0, 1, 0), Retention::Expired);
        // Exactly now (age 0) is still within a zero window.
        assert_eq!(classify_retention(0, 0, 0), Retention::Keep);
    }

    #[test]
    fn non_monotonic_now_does_not_wrap() {
        // now earlier than recorded_at -> age saturates to 0 -> kept.
        assert_eq!(classify_retention(100, 50, 7), Retention::Keep);
    }
}
