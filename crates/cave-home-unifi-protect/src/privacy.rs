//! Privacy mode / privacy schedule (Charter §9).
//!
//! Charter §9 is privacy-first: a household must be able to say "this camera is
//! off while we are home" and have the system honour it. UniFi Protect models
//! this as a privacy zone / privacy mode that can mask or disable a camera.
//! cave-home expresses it as a [`PrivacySchedule`]: a daily window during which
//! the camera is masked (no recording, no notifying) — *except* for life-safety
//! alarms, which are never suppressed.
//!
//! This is pure logic over a caller-supplied wall-clock [`TimeOfDay`]; the crate
//! reads no real clock.

/// A wall-clock time of day, to the minute, in 24-hour form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TimeOfDay {
    minutes: u16,
}

/// Why a [`TimeOfDay`] could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeError {
    /// Hour was not in `0..=23`.
    BadHour,
    /// Minute was not in `0..=59`.
    BadMinute,
}

impl core::fmt::Display for TimeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BadHour => f.write_str("hour must be 0..=23"),
            Self::BadMinute => f.write_str("minute must be 0..=59"),
        }
    }
}

impl std::error::Error for TimeError {}

impl TimeOfDay {
    /// Build a time of day from a 24-hour `hour` and `minute`.
    ///
    /// # Errors
    /// Returns [`TimeError`] if `hour > 23` or `minute > 59`.
    pub const fn new(hour: u8, minute: u8) -> Result<Self, TimeError> {
        if hour > 23 {
            return Err(TimeError::BadHour);
        }
        if minute > 59 {
            return Err(TimeError::BadMinute);
        }
        Ok(Self {
            minutes: hour as u16 * 60 + minute as u16,
        })
    }

    /// Minutes since midnight, `0..=1439`.
    #[must_use]
    pub const fn minutes(self) -> u16 {
        self.minutes
    }
}

/// Whether a camera is currently masked by privacy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyState {
    /// The camera sees and records normally.
    Active,
    /// The camera is masked: no recording, no notifications (except life-safety
    /// alarms, which [`PrivacySchedule::allows`] never suppresses).
    Masked,
}

impl PrivacyState {
    /// Whether the camera is masked right now.
    #[must_use]
    pub const fn is_masked(self) -> bool {
        matches!(self, Self::Masked)
    }
}

/// A daily privacy window during which a camera is masked.
///
/// `start > end` is read as an overnight window, the same convention as
/// [`crate::recording::Schedule`]. A schedule can also be `enabled = false`,
/// meaning the household has the feature configured but currently switched off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrivacySchedule {
    start: TimeOfDay,
    end: TimeOfDay,
    enabled: bool,
}

impl PrivacySchedule {
    /// A privacy window `[start, end]` (inclusive), enabled.
    #[must_use]
    pub const fn new(start: TimeOfDay, end: TimeOfDay) -> Self {
        Self {
            start,
            end,
            enabled: true,
        }
    }

    /// Turn the schedule on or off without losing the window.
    #[must_use]
    pub const fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Whether the schedule is currently switched on.
    #[must_use]
    pub const fn is_enabled(self) -> bool {
        self.enabled
    }

    /// The privacy state at `now`: [`PrivacyState::Masked`] if the schedule is
    /// enabled and `now` falls inside the window, else [`PrivacyState::Active`].
    #[must_use]
    pub fn state_at(self, now: TimeOfDay) -> PrivacyState {
        if !self.enabled {
            return PrivacyState::Active;
        }
        let (s, e, n) = (self.start.minutes(), self.end.minutes(), now.minutes());
        let inside = if s <= e {
            n >= s && n <= e
        } else {
            n >= s || n <= e
        };
        if inside {
            PrivacyState::Masked
        } else {
            PrivacyState::Active
        }
    }

    /// Whether a detection at `now` is allowed to record / notify.
    ///
    /// A life-safety alarm (`is_safety_alarm`) is **always** allowed — privacy
    /// never silences a smoke or CO alarm (Charter §9 privacy must not defeat
    /// safety). Any other detection is allowed only when the camera is not
    /// masked.
    #[must_use]
    pub fn allows(self, now: TimeOfDay, is_safety_alarm: bool) -> bool {
        if is_safety_alarm {
            return true;
        }
        !self.state_at(now).is_masked()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(h: u8, m: u8) -> TimeOfDay {
        TimeOfDay::new(h, m).expect("valid time")
    }

    #[test]
    fn time_of_day_rejects_bad_values() {
        assert_eq!(TimeOfDay::new(24, 0), Err(TimeError::BadHour));
        assert_eq!(TimeOfDay::new(0, 60), Err(TimeError::BadMinute));
        assert_eq!(t(13, 30).minutes(), 13 * 60 + 30);
    }

    #[test]
    fn masked_inside_a_daytime_window() {
        let p = PrivacySchedule::new(t(8, 0), t(18, 0));
        assert_eq!(p.state_at(t(12, 0)), PrivacyState::Masked);
        assert_eq!(p.state_at(t(7, 59)), PrivacyState::Active);
        assert_eq!(p.state_at(t(18, 0)), PrivacyState::Masked);
        assert_eq!(p.state_at(t(18, 1)), PrivacyState::Active);
    }

    #[test]
    fn overnight_privacy_window_wraps() {
        let p = PrivacySchedule::new(t(23, 0), t(5, 0));
        assert_eq!(p.state_at(t(2, 0)), PrivacyState::Masked);
        assert_eq!(p.state_at(t(23, 30)), PrivacyState::Masked);
        assert_eq!(p.state_at(t(12, 0)), PrivacyState::Active);
    }

    #[test]
    fn disabled_schedule_never_masks() {
        let p = PrivacySchedule::new(t(0, 0), t(23, 59)).with_enabled(false);
        assert!(!p.is_enabled());
        assert_eq!(p.state_at(t(12, 0)), PrivacyState::Active);
    }

    #[test]
    fn ordinary_detection_blocked_while_masked() {
        let p = PrivacySchedule::new(t(8, 0), t(18, 0));
        assert!(!p.allows(t(12, 0), false));
        assert!(p.allows(t(20, 0), false));
    }

    #[test]
    fn safety_alarm_always_allowed_even_while_masked() {
        let p = PrivacySchedule::new(t(0, 0), t(23, 59));
        assert!(p.allows(t(12, 0), true));
    }

    #[test]
    fn privacy_state_helper() {
        assert!(PrivacyState::Masked.is_masked());
        assert!(!PrivacyState::Active.is_masked());
    }
}
