//! The [`Position`] value object — a validated 0..=100 percentage.
//!
//! A cover's openness and its tilt are both expressed as a whole-number
//! percentage from 0 (fully closed / slats shut) to 100 (fully open / slats
//! flat). Using a `u8` makes the `NaN`/infinite hazard of a float impossible by
//! construction; the only thing left to guard is the upper bound.

/// A cover position as a whole-number percentage, 0..=100.
///
/// 0 means fully closed, 100 means fully open. Construction clamps or rejects
/// out-of-range input depending on which constructor you choose, so nothing
/// downstream ever has to defend against a 137-percent-open blind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position(u8);

/// Why a [`Position`] could not be constructed from an exact value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionError {
    /// The percentage was above 100.
    OutOfRange,
}

impl core::fmt::Display for PositionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::OutOfRange => f.write_str("position percentage must be 0..=100"),
        }
    }
}

impl std::error::Error for PositionError {}

impl Position {
    /// Fully closed (0%).
    pub const CLOSED: Self = Self(0);
    /// Fully open (100%).
    pub const OPEN: Self = Self(100);

    /// Construct a position, rejecting anything above 100%.
    ///
    /// # Errors
    /// Returns [`PositionError::OutOfRange`] if `percent` exceeds 100.
    pub const fn new(percent: u8) -> Result<Self, PositionError> {
        if percent > 100 {
            return Err(PositionError::OutOfRange);
        }
        Ok(Self(percent))
    }

    /// Construct a position, clamping anything above 100% down to 100.
    ///
    /// Useful when accepting a target from a slider that may overshoot; the
    /// physical cover can never be more than fully open.
    #[must_use]
    pub const fn clamped(percent: u8) -> Self {
        if percent > 100 {
            Self(100)
        } else {
            Self(percent)
        }
    }

    /// The percentage value, 0..=100.
    #[must_use]
    pub const fn percent(self) -> u8 {
        self.0
    }

    /// Whether the cover is fully closed.
    #[must_use]
    pub const fn is_closed(self) -> bool {
        self.0 == 0
    }

    /// Whether the cover is fully open.
    #[must_use]
    pub const fn is_open(self) -> bool {
        self.0 == 100
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_accepts_full_range() {
        assert_eq!(Position::new(0).unwrap().percent(), 0);
        assert_eq!(Position::new(50).unwrap().percent(), 50);
        assert_eq!(Position::new(100).unwrap().percent(), 100);
    }

    #[test]
    fn new_rejects_above_hundred() {
        assert_eq!(Position::new(101), Err(PositionError::OutOfRange));
        assert_eq!(Position::new(255), Err(PositionError::OutOfRange));
    }

    #[test]
    fn clamped_caps_at_hundred() {
        assert_eq!(Position::clamped(101).percent(), 100);
        assert_eq!(Position::clamped(200).percent(), 100);
        assert_eq!(Position::clamped(73).percent(), 73);
    }

    #[test]
    fn closed_and_open_constants_and_predicates() {
        assert!(Position::CLOSED.is_closed());
        assert!(!Position::CLOSED.is_open());
        assert!(Position::OPEN.is_open());
        assert!(!Position::OPEN.is_closed());
        assert!(!Position::new(50).unwrap().is_open());
        assert!(!Position::new(50).unwrap().is_closed());
    }

    #[test]
    fn ordering_follows_openness() {
        assert!(Position::CLOSED < Position::new(1).unwrap());
        assert!(Position::new(40).unwrap() < Position::new(60).unwrap());
        assert!(Position::new(99).unwrap() < Position::OPEN);
    }
}
