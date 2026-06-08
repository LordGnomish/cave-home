//! Time-of-day routing schedule.
//!
//! A household decides when calls should ring through and when they should go
//! straight to voicemail (or a forward): "ring the house from 8 in the morning
//! to 10 at night; after that, take a message". This module models that single
//! business-hours window. Like the rest of the crate it reads no clock — the
//! caller supplies the current wall-clock [`Minute`] of the day.
//!
//! The window may wrap past midnight (e.g. a night-shift household reachable
//! 22:00–06:00), handled the same way the doorbell quiet-hours window does.

/// A wall-clock minute of the day, `0..=1439` (00:00 = 0, 23:59 = 1439).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Minute(u16);

/// Why a [`Minute`] could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinuteError {
    /// The value was not in `0..=1439`.
    OutOfRange,
}

impl core::fmt::Display for MinuteError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::OutOfRange => f.write_str("minute of day out of range"),
        }
    }
}

impl std::error::Error for MinuteError {}

impl Minute {
    /// Construct from a minute-of-day count (`0..=1439`).
    ///
    /// # Errors
    /// [`MinuteError::OutOfRange`] if `minute_of_day > 1439`.
    pub const fn new(minute_of_day: u16) -> Result<Self, MinuteError> {
        if minute_of_day <= 1439 {
            Ok(Self(minute_of_day))
        } else {
            Err(MinuteError::OutOfRange)
        }
    }

    /// Construct from a 24-hour clock time.
    ///
    /// # Errors
    /// [`MinuteError::OutOfRange`] if `hour > 23` or `minute > 59`.
    pub const fn at(hour: u8, minute: u8) -> Result<Self, MinuteError> {
        if hour <= 23 && minute <= 59 {
            Ok(Self(hour as u16 * 60 + minute as u16))
        } else {
            Err(MinuteError::OutOfRange)
        }
    }

    #[must_use]
    pub const fn minute_of_day(self) -> u16 {
        self.0
    }
}

/// The window during which calls ring through. Outside it, a call is "after
/// hours" and routing sends it to voicemail / forward instead of ringing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BusinessHours {
    start: Minute,
    end: Minute,
}

impl BusinessHours {
    /// A window `[start, end)`. If `start == end` the window is treated as
    /// *always open* (a household that never goes to after-hours mode).
    ///
    /// If `end` is earlier than `start` the window wraps past midnight
    /// (e.g. 22:00 → 06:00 covers the night).
    #[must_use]
    pub const fn new(start: Minute, end: Minute) -> Self {
        Self { start, end }
    }

    /// A window that is open all day — every call rings through.
    #[must_use]
    pub const fn always_open() -> Self {
        // start == end ⇒ always open (see `is_open`).
        Self { start: Minute(0), end: Minute(0) }
    }

    /// Whether calls ring through at wall-clock time `now`.
    ///
    /// Half-open `[start, end)`: a call exactly at `end` is already after hours.
    #[must_use]
    pub const fn is_open(self, now: Minute) -> bool {
        let s = self.start.0;
        let e = self.end.0;
        let n = now.0;
        if s == e {
            // Degenerate window ⇒ always open.
            true
        } else if s < e {
            // Normal same-day window.
            n >= s && n < e
        } else {
            // Wraps past midnight: open if at/after start OR before end.
            n >= s || n < e
        }
    }

    /// Whether `now` is after hours (the inverse of [`BusinessHours::is_open`]).
    #[must_use]
    pub const fn is_after_hours(self, now: Minute) -> bool {
        !self.is_open(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(h: u8, min: u8) -> Minute {
        Minute::at(h, min).unwrap()
    }

    #[test]
    fn minute_validates_range() {
        assert!(Minute::new(0).is_ok());
        assert!(Minute::new(1439).is_ok());
        assert_eq!(Minute::new(1440), Err(MinuteError::OutOfRange));
        assert_eq!(Minute::at(24, 0), Err(MinuteError::OutOfRange));
        assert_eq!(Minute::at(12, 60), Err(MinuteError::OutOfRange));
    }

    #[test]
    fn minute_of_day_from_clock() {
        assert_eq!(Minute::at(0, 0).unwrap().minute_of_day(), 0);
        assert_eq!(Minute::at(8, 30).unwrap().minute_of_day(), 510);
        assert_eq!(Minute::at(23, 59).unwrap().minute_of_day(), 1439);
    }

    #[test]
    fn same_day_window_is_open_within_bounds() {
        let bh = BusinessHours::new(m(8, 0), m(22, 0));
        assert!(!bh.is_open(m(7, 59)));
        assert!(bh.is_open(m(8, 0)), "start is inclusive");
        assert!(bh.is_open(m(14, 0)));
        assert!(bh.is_open(m(21, 59)));
        assert!(!bh.is_open(m(22, 0)), "end is exclusive");
        assert!(bh.is_after_hours(m(23, 30)));
    }

    #[test]
    fn wrapping_window_covers_midnight() {
        // Night-shift household: reachable 22:00 → 06:00.
        let bh = BusinessHours::new(m(22, 0), m(6, 0));
        assert!(bh.is_open(m(23, 0)));
        assert!(bh.is_open(m(0, 0)));
        assert!(bh.is_open(m(5, 59)));
        assert!(!bh.is_open(m(6, 0)), "end is exclusive even across midnight");
        assert!(bh.is_after_hours(m(12, 0)));
    }

    #[test]
    fn always_open_window_rings_at_any_time() {
        let bh = BusinessHours::always_open();
        assert!(bh.is_open(m(3, 0)));
        assert!(bh.is_open(m(15, 0)));
        assert!(!bh.is_after_hours(m(23, 59)));
    }
}
