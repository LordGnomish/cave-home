// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Access schedules (ADR-009).
//!
//! A [`Schedule`] is a set of allowed time windows expressed in *minute-of-week*
//! (0 = Monday 00:00, 10079 = Sunday 23:59). The caller supplies the current
//! minute-of-week; this module decides whether that instant falls inside an
//! allowed window. Windows may **wrap past the end of the week** (e.g. a
//! Sunday-23:00 → Monday-06:00 night window), which is handled explicitly.

/// Minutes in a full week (7 × 24 × 60).
pub const MINUTES_PER_WEEK: u32 = 7 * 24 * 60;

/// A single allowed window, half-open `[start, end)` in minute-of-week.
///
/// If `end <= start` the window is treated as **wrapping** past the end of the
/// week: it covers `[start, MINUTES_PER_WEEK)` ∪ `[0, end)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    start: u32,
    end: u32,
}

impl Window {
    /// Construct a window from two minute-of-week values, each clamped into
    /// `0..MINUTES_PER_WEEK`.
    #[must_use]
    pub fn new(start: u32, end: u32) -> Self {
        Self {
            start: start % MINUTES_PER_WEEK,
            end: end % MINUTES_PER_WEEK,
        }
    }

    /// Whether this window wraps past the end of the week.
    #[must_use]
    pub fn wraps(&self) -> bool {
        self.end <= self.start
    }

    /// Whether the given minute-of-week falls inside this window.
    #[must_use]
    pub fn contains(&self, minute_of_week: u32) -> bool {
        let m = minute_of_week % MINUTES_PER_WEEK;
        if self.start == self.end {
            // Degenerate zero-length window: matches nothing.
            false
        } else if self.wraps() {
            m >= self.start || m < self.end
        } else {
            m >= self.start && m < self.end
        }
    }
}

/// A set of allowed windows. Empty means "never allowed"; a schedule that is
/// [`Schedule::always`] is allowed at every instant.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Schedule {
    windows: Vec<Window>,
    /// When set, the schedule allows every instant regardless of windows.
    always: bool,
}

impl Schedule {
    /// An empty schedule: never allowed.
    #[must_use]
    pub fn never() -> Self {
        Self {
            windows: Vec::new(),
            always: false,
        }
    }

    /// A schedule that allows every instant (e.g. the owner's 24/7 access).
    #[must_use]
    pub fn always() -> Self {
        Self {
            windows: Vec::new(),
            always: true,
        }
    }

    /// Build a schedule from a list of windows.
    #[must_use]
    pub fn from_windows(windows: Vec<Window>) -> Self {
        Self {
            windows,
            always: false,
        }
    }

    /// Add a window (builder-style).
    #[must_use]
    pub fn with_window(mut self, window: Window) -> Self {
        self.windows.push(window);
        self
    }

    /// Whether this schedule always allows access.
    #[must_use]
    pub fn is_always(&self) -> bool {
        self.always
    }

    /// Whether the given minute-of-week is inside any allowed window.
    #[must_use]
    pub fn allows(&self, minute_of_week: u32) -> bool {
        if self.always {
            return true;
        }
        self.windows.iter().any(|w| w.contains(minute_of_week))
    }
}

/// Convenience: build a minute-of-week from a weekday index (0 = Monday) and a
/// time of day. Out-of-range values wrap into the valid range.
#[must_use]
pub fn minute_of_week(weekday_mon0: u32, hour: u32, minute: u32) -> u32 {
    let day = weekday_mon0 % 7;
    let h = hour % 24;
    let m = minute % 60;
    (day * 24 * 60) + (h * 60) + m
}

#[cfg(test)]
mod tests {
    use super::*;

    // Monday 09:00.
    const MON_0900: u32 = 9 * 60;
    // Monday 17:00.
    const MON_1700: u32 = 17 * 60;

    #[test]
    fn minute_of_week_helper() {
        assert_eq!(minute_of_week(0, 0, 0), 0); // Mon 00:00
        assert_eq!(minute_of_week(0, 9, 0), MON_0900);
        assert_eq!(minute_of_week(6, 23, 59), MINUTES_PER_WEEK - 1); // Sun 23:59
    }

    #[test]
    fn simple_window_contains() {
        let w = Window::new(MON_0900, MON_1700);
        assert!(!w.wraps());
        assert!(w.contains(MON_0900)); // inclusive start
        assert!(w.contains(MON_0900 + 60));
        assert!(!w.contains(MON_1700)); // exclusive end
        assert!(!w.contains(MON_0900 - 1)); // before
    }

    #[test]
    fn window_start_boundary_inclusive_end_exclusive() {
        let w = Window::new(100, 200);
        assert!(w.contains(100));
        assert!(w.contains(199));
        assert!(!w.contains(200));
        assert!(!w.contains(99));
    }

    #[test]
    fn wrapping_window_across_week_boundary() {
        // Sunday 23:00 -> Monday 06:00.
        let sun_2300 = minute_of_week(6, 23, 0);
        let mon_0600 = minute_of_week(0, 6, 0);
        let w = Window::new(sun_2300, mon_0600);
        assert!(w.wraps());
        assert!(w.contains(sun_2300)); // start
        assert!(w.contains(MINUTES_PER_WEEK - 1)); // Sun 23:59 — late side
        assert!(w.contains(0)); // Mon 00:00 — early side
        assert!(w.contains(mon_0600 - 1)); // Mon 05:59
        assert!(!w.contains(mon_0600)); // Mon 06:00 — exclusive end
        assert!(!w.contains(minute_of_week(2, 12, 0))); // Wed noon — outside
    }

    #[test]
    fn zero_length_window_matches_nothing() {
        let w = Window::new(500, 500);
        assert!(!w.contains(500));
        assert!(!w.contains(499));
    }

    #[test]
    fn schedule_never_allows_nothing() {
        let s = Schedule::never();
        assert!(!s.allows(0));
        assert!(!s.allows(MON_0900));
    }

    #[test]
    fn schedule_always_allows_everything() {
        let s = Schedule::always();
        assert!(s.is_always());
        assert!(s.allows(0));
        assert!(s.allows(MINUTES_PER_WEEK - 1));
    }

    #[test]
    fn schedule_unions_its_windows() {
        let morning = Window::new(minute_of_week(0, 8, 0), minute_of_week(0, 12, 0));
        let evening = Window::new(minute_of_week(0, 18, 0), minute_of_week(0, 22, 0));
        let s = Schedule::from_windows(vec![morning]).with_window(evening);
        assert!(s.allows(minute_of_week(0, 9, 0))); // morning
        assert!(s.allows(minute_of_week(0, 20, 0))); // evening
        assert!(!s.allows(minute_of_week(0, 14, 0))); // afternoon gap
    }

    #[test]
    fn input_minute_is_wrapped_into_range() {
        let w = Window::new(0, 60);
        // MINUTES_PER_WEEK maps back to 0, which is inside [0,60).
        assert!(w.contains(MINUTES_PER_WEEK));
    }
}
