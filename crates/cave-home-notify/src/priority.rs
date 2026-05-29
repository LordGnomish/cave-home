//! Notification priority — how loudly a message asks for attention.
//!
//! Five levels, ordered least → most urgent. The ordering is the whole point:
//! a subscriber sets a *minimum* priority (see [`crate::route`]) and only
//! messages at or above it get through. The numeric mapping (1..=5) mirrors the
//! ntfy-class priority convention, but the type — and its grandma-friendly
//! labels — is first-party (ADR-021): no upstream source was read.

/// How urgent a notification is, ordered least → most urgent.
///
/// `Min < Low < Default < High < Max`, so a subscriber's minimum-priority
/// filter is a simple `>=` comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Priority {
    /// Barely worth a buzz — collect-them-later background notes.
    Min,
    /// Low importance — a quiet note.
    Low,
    /// The normal level when the caller says nothing.
    #[default]
    Default,
    /// Worth interrupting for — an alert.
    High,
    /// The most urgent level — should wake someone up.
    Max,
}

impl Priority {
    /// All five levels, least → most urgent. Handy for exhaustive iteration.
    pub const ALL: [Self; 5] = [
        Self::Min,
        Self::Low,
        Self::Default,
        Self::High,
        Self::Max,
    ];

    /// The 1..=5 numeric level, matching the ntfy-class convention.
    #[must_use]
    pub const fn level(self) -> u8 {
        match self {
            Self::Min => 1,
            Self::Low => 2,
            Self::Default => 3,
            Self::High => 4,
            Self::Max => 5,
        }
    }

    /// Build a priority from its 1..=5 numeric level.
    ///
    /// Anything outside 1..=5 is `None` — the caller decides whether to fall
    /// back to [`Priority::Default`] or reject.
    #[must_use]
    pub const fn from_level(level: u8) -> Option<Self> {
        match level {
            1 => Some(Self::Min),
            2 => Some(Self::Low),
            3 => Some(Self::Default),
            4 => Some(Self::High),
            5 => Some(Self::Max),
            _ => None,
        }
    }

    /// Whether a message of this priority clears a subscriber's `minimum`.
    #[must_use]
    pub const fn meets(self, minimum: Self) -> bool {
        self.level() >= minimum.level()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_is_least_to_most_urgent() {
        assert!(Priority::Min < Priority::Low);
        assert!(Priority::Low < Priority::Default);
        assert!(Priority::Default < Priority::High);
        assert!(Priority::High < Priority::Max);
    }

    #[test]
    fn default_is_the_middle_level() {
        assert_eq!(Priority::default(), Priority::Default);
        assert_eq!(Priority::default().level(), 3);
    }

    #[test]
    fn level_round_trips_through_from_level() {
        for p in Priority::ALL {
            assert_eq!(Priority::from_level(p.level()), Some(p));
        }
    }

    #[test]
    fn from_level_rejects_out_of_range() {
        assert_eq!(Priority::from_level(0), None);
        assert_eq!(Priority::from_level(6), None);
        assert_eq!(Priority::from_level(u8::MAX), None);
    }

    #[test]
    fn meets_is_at_or_above_minimum() {
        // High clears a High floor and a Default floor, but not a Max floor.
        assert!(Priority::High.meets(Priority::High));
        assert!(Priority::High.meets(Priority::Default));
        assert!(!Priority::High.meets(Priority::Max));
        // Min only ever clears a Min floor.
        assert!(Priority::Min.meets(Priority::Min));
        assert!(!Priority::Min.meets(Priority::Low));
    }
}
