//! Growing-season / dormancy flag — pure, from a caller-supplied month.
//!
//! cave-home-garden owns no clock. The caller passes the current month as an
//! integer `1..=12` (1 = January). This module says whether a plant is in its
//! **growing season** or **dormant**, so the care engine can soften its advice
//! out of season (a dormant plant wants far less water, and "needs water" alarms
//! in deep winter are usually noise).
//!
//! # Hemisphere
//!
//! The default is the **northern hemisphere**: the growing season runs
//! March–October (months 3..=10), dormancy November–February. For the southern
//! hemisphere the calendar is shifted by six months. Pass the
//! [`Hemisphere`] explicitly; [`Hemisphere::Northern`] is the documented
//! default via [`growing_season`].

use crate::label::Lang;

/// Which hemisphere the garden is in — flips the growing-season calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Hemisphere {
    /// Growing season March–October. The documented default.
    #[default]
    Northern,
    /// Growing season September–April.
    Southern,
}

/// Whether a plant is actively growing or resting this month.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Season {
    /// Actively growing — full care applies.
    Growing,
    /// Resting — needs much less water and tolerates being left alone.
    Dormant,
}

impl Season {
    /// Whether this is the active growing season.
    #[must_use]
    pub const fn is_growing(self) -> bool {
        matches!(self, Self::Growing)
    }

    /// A plain-language note for the household (Charter §6.3 — "resting for the
    /// winter", never "dormancy cycle").
    #[must_use]
    pub const fn message(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Growing, Lang::En) => "The garden is growing.",
            (Self::Growing, Lang::De) => "Der Garten wächst.",
            (Self::Growing, Lang::Tr) => "Bahçe büyüyor.",
            (Self::Dormant, Lang::En) => "The garden is resting for the winter.",
            (Self::Dormant, Lang::De) => "Der Garten ruht über den Winter.",
            (Self::Dormant, Lang::Tr) => "Bahçe kış için dinleniyor.",
        }
    }
}

/// The growing season for a month (`1..=12`) in the northern hemisphere.
///
/// Months outside `1..=12` are treated as dormant (defensive — there is no
/// valid 13th month to grow in), so the function is total and never panics.
#[must_use]
pub fn growing_season(month: u8) -> Season {
    growing_season_in(month, Hemisphere::Northern)
}

/// The growing season for a month (`1..=12`) in a given hemisphere.
#[must_use]
pub fn growing_season_in(month: u8, hemisphere: Hemisphere) -> Season {
    let growing = match hemisphere {
        // Northern: March (3) … October (10) inclusive.
        Hemisphere::Northern => (3..=10).contains(&month),
        // Southern: September (9) … December (12) and January (1) … April (4).
        Hemisphere::Southern => matches!(month, 9..=12 | 1..=4),
    };
    if growing {
        Season::Growing
    } else {
        Season::Dormant
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn northern_growing_months() {
        for m in 3..=10 {
            assert_eq!(growing_season(m), Season::Growing, "month {m} northern");
        }
        for m in [1, 2, 11, 12] {
            assert_eq!(growing_season(m), Season::Dormant, "month {m} northern");
        }
    }

    #[test]
    fn northern_boundaries() {
        assert_eq!(growing_season(2), Season::Dormant);
        assert_eq!(growing_season(3), Season::Growing); // start
        assert_eq!(growing_season(10), Season::Growing); // end
        assert_eq!(growing_season(11), Season::Dormant);
    }

    #[test]
    fn southern_is_offset() {
        // Southern-hemisphere summer is December–February.
        assert_eq!(growing_season_in(1, Hemisphere::Southern), Season::Growing);
        assert_eq!(growing_season_in(12, Hemisphere::Southern), Season::Growing);
        // Southern winter is June–August.
        assert_eq!(growing_season_in(7, Hemisphere::Southern), Season::Dormant);
        // And it disagrees with the north in July.
        assert_eq!(growing_season_in(7, Hemisphere::Northern), Season::Growing);
    }

    #[test]
    fn default_hemisphere_is_northern() {
        assert_eq!(Hemisphere::default(), Hemisphere::Northern);
        assert_eq!(growing_season(7), growing_season_in(7, Hemisphere::default()));
    }

    #[test]
    fn out_of_range_month_is_dormant_not_panic() {
        assert_eq!(growing_season(0), Season::Dormant);
        assert_eq!(growing_season(13), Season::Dormant);
        assert_eq!(growing_season(u8::MAX), Season::Dormant);
    }

    #[test]
    fn is_growing_helper() {
        assert!(Season::Growing.is_growing());
        assert!(!Season::Dormant.is_growing());
    }
}
