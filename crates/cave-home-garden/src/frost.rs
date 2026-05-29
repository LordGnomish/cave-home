//! Frost-risk classification — pure temperature thresholds.
//!
//! Given the **forecast minimum temperature** (the caller supplies it; this
//! crate owns no weather feed) and a plant's [`FrostSensitivity`], classify the
//! overnight cold risk into one of four levels. The thresholds are deliberately
//! simple and conservative — a household acts on "cover the tender plants
//! tonight", not on a decimal degree.
//!
//! Thresholds (forecast min temperature, °C):
//!
//! | sensitivity   | None    | Watch        | Warning      | Danger   |
//! |---------------|---------|--------------|--------------|----------|
//! | `Tender`      | > +4    | +2 … +4      | 0 … +2       | < 0      |
//! | `HalfHardy`   | > +1    | -1 … +1      | -3 … -1      | < -3     |
//! | `Hardy`       | > -2    | -5 … -2      | -8 … -5      | < -8     |
//!
//! Ranges are interpreted "warmer bound exclusive, colder bound inclusive" so a
//! temperature on a boundary falls into the *colder* (more cautious) band.

use crate::label::Lang;
use crate::plant::FrostSensitivity;

/// The overnight frost risk for a plant, coldest-last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FrostRisk {
    /// No meaningful cold risk tonight.
    None,
    /// Getting chilly — worth keeping an eye on.
    Watch,
    /// Frost likely — protect tender plants.
    Warning,
    /// Hard freeze — unprotected tender plants will be damaged.
    Danger,
}

impl FrostRisk {
    /// Classify the overnight risk from a forecast minimum and a plant's
    /// frost sensitivity. Pure thresholds — see the module table.
    #[must_use]
    pub fn classify(forecast_min_c: f64, sensitivity: FrostSensitivity) -> Self {
        // (watch_below, warning_below, danger_below): warmer→colder boundaries.
        let (watch, warning, danger) = match sensitivity {
            FrostSensitivity::Tender => (4.0, 2.0, 0.0),
            FrostSensitivity::HalfHardy => (1.0, -1.0, -3.0),
            FrostSensitivity::Hardy => (-2.0, -5.0, -8.0),
        };
        if forecast_min_c <= danger {
            Self::Danger
        } else if forecast_min_c <= warning {
            Self::Warning
        } else if forecast_min_c <= watch {
            Self::Watch
        } else {
            Self::None
        }
    }

    /// Whether this risk warrants any household action at all.
    #[must_use]
    pub const fn needs_action(self) -> bool {
        !matches!(self, Self::None)
    }

    /// A plain-language alert for the household (Charter §6.3 — "frost",
    /// "cover the plants", never a temperature or a sensitivity class).
    #[must_use]
    pub const fn message(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::None, Lang::En) => "No frost expected tonight.",
            (Self::None, Lang::De) => "Heute Nacht kein Frost erwartet.",
            (Self::None, Lang::Tr) => "Bu gece don beklenmiyor.",
            (Self::Watch, Lang::En) => "Getting chilly tonight — keep an eye on the tender plants.",
            (Self::Watch, Lang::De) => {
                "Heute Nacht wird es kühl — die empfindlichen Pflanzen im Auge behalten."
            }
            (Self::Watch, Lang::Tr) => "Bu gece serinliyor — nazik bitkilere göz kulak olun.",
            (Self::Warning, Lang::En) => "Frost tonight — cover the tender plants.",
            (Self::Warning, Lang::De) => "Heute Nacht Frost — die empfindlichen Pflanzen abdecken.",
            (Self::Warning, Lang::Tr) => "Bu gece don var — nazik bitkileri örtün.",
            (Self::Danger, Lang::En) => "Hard freeze tonight — bring tender plants inside or cover them well.",
            (Self::Danger, Lang::De) => {
                "Heute Nacht starker Frost — empfindliche Pflanzen hereinholen oder gut abdecken."
            }
            (Self::Danger, Lang::Tr) => {
                "Bu gece sert don var — nazik bitkileri içeri alın ya da iyice örtün."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tender_thresholds() {
        use FrostSensitivity::Tender;
        assert_eq!(FrostRisk::classify(5.0, Tender), FrostRisk::None);
        assert_eq!(FrostRisk::classify(4.0, Tender), FrostRisk::Watch); // boundary → colder band
        assert_eq!(FrostRisk::classify(3.0, Tender), FrostRisk::Watch);
        assert_eq!(FrostRisk::classify(2.0, Tender), FrostRisk::Warning);
        assert_eq!(FrostRisk::classify(1.0, Tender), FrostRisk::Warning);
        assert_eq!(FrostRisk::classify(0.0, Tender), FrostRisk::Danger);
        assert_eq!(FrostRisk::classify(-3.0, Tender), FrostRisk::Danger);
    }

    #[test]
    fn half_hardy_thresholds() {
        use FrostSensitivity::HalfHardy;
        assert_eq!(FrostRisk::classify(3.0, HalfHardy), FrostRisk::None);
        assert_eq!(FrostRisk::classify(1.0, HalfHardy), FrostRisk::Watch);
        assert_eq!(FrostRisk::classify(0.0, HalfHardy), FrostRisk::Watch);
        assert_eq!(FrostRisk::classify(-1.0, HalfHardy), FrostRisk::Warning);
        assert_eq!(FrostRisk::classify(-3.0, HalfHardy), FrostRisk::Danger);
        assert_eq!(FrostRisk::classify(-5.0, HalfHardy), FrostRisk::Danger);
    }

    #[test]
    fn hardy_thresholds() {
        use FrostSensitivity::Hardy;
        assert_eq!(FrostRisk::classify(0.0, Hardy), FrostRisk::None);
        assert_eq!(FrostRisk::classify(-2.0, Hardy), FrostRisk::Watch);
        assert_eq!(FrostRisk::classify(-5.0, Hardy), FrostRisk::Warning);
        assert_eq!(FrostRisk::classify(-8.0, Hardy), FrostRisk::Danger);
        assert_eq!(FrostRisk::classify(-12.0, Hardy), FrostRisk::Danger);
    }

    #[test]
    fn the_same_cold_night_hits_tender_hardest() {
        // A 0 °C night: danger for tender, watch for half-hardy, fine for hardy.
        assert_eq!(FrostRisk::classify(0.0, FrostSensitivity::Tender), FrostRisk::Danger);
        assert_eq!(FrostRisk::classify(0.0, FrostSensitivity::HalfHardy), FrostRisk::Watch);
        assert_eq!(FrostRisk::classify(0.0, FrostSensitivity::Hardy), FrostRisk::None);
    }

    #[test]
    fn ordering_and_needs_action() {
        assert!(FrostRisk::None < FrostRisk::Watch);
        assert!(FrostRisk::Warning < FrostRisk::Danger);
        assert!(!FrostRisk::None.needs_action());
        assert!(FrostRisk::Watch.needs_action());
        assert!(FrostRisk::Danger.needs_action());
    }
}
