//! Hoymiles microinverter family model.
//!
//! Clean-room (Charter §6.1 / ADR-002): the channel counts below are taken
//! from Hoymiles' **public product datasheets** (how many solar panels each
//! model accepts), not from any GPL source.
//!
//! A Hoymiles microinverter sits behind one or more solar panels. The model
//! number encodes how many independent DC inputs (panels) it has, which in
//! turn fixes how many per-panel readings a telemetry payload carries:
//!
//! | Family            | Example models      | Panels (DC channels) |
//! |-------------------|---------------------|----------------------|
//! | One-panel         | HM-300 / 350 / 400  | 1                    |
//! | Two-panel         | HM-600 / 700 / 800  | 2                    |
//! | Four-panel        | HM-1200 / 1500      | 4                    |
//!
//! All families report a single AC (grid) side.

/// A Hoymiles microinverter family, distinguished by how many solar panels it
/// drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// HM-300 / HM-350 / HM-400 — a single panel.
    OnePanel,
    /// HM-600 / HM-700 / HM-800 — two panels.
    TwoPanel,
    /// HM-1200 / HM-1500 — four panels.
    FourPanel,
}

impl Family {
    /// Number of independent solar-panel (DC) inputs this family has.
    #[must_use]
    pub const fn panel_count(self) -> usize {
        match self {
            Self::OnePanel => 1,
            Self::TwoPanel => 2,
            Self::FourPanel => 4,
        }
    }

    /// Infer the family from the inverter's rated AC output in watts.
    ///
    /// Returns [`None`] for a wattage that does not match a known Hoymiles HM
    /// rating, so callers can reject an unrecognised inverter rather than
    /// guessing a panel count.
    #[must_use]
    pub const fn from_rated_watts(watts: u16) -> Option<Self> {
        match watts {
            300 | 350 | 400 => Some(Self::OnePanel),
            600 | 700 | 800 => Some(Self::TwoPanel),
            1200 | 1500 => Some(Self::FourPanel),
            _ => None,
        }
    }

    /// A short, end-user-facing description (Charter §6.3 — no model-number
    /// jargon, just the panel count a household understands).
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::OnePanel => "single-panel inverter",
            Self::TwoPanel => "two-panel inverter",
            Self::FourPanel => "four-panel inverter",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_counts_match_datasheet() {
        assert_eq!(Family::OnePanel.panel_count(), 1);
        assert_eq!(Family::TwoPanel.panel_count(), 2);
        assert_eq!(Family::FourPanel.panel_count(), 4);
    }

    #[test]
    fn rated_watts_map_to_families() {
        assert_eq!(Family::from_rated_watts(300), Some(Family::OnePanel));
        assert_eq!(Family::from_rated_watts(400), Some(Family::OnePanel));
        assert_eq!(Family::from_rated_watts(600), Some(Family::TwoPanel));
        assert_eq!(Family::from_rated_watts(800), Some(Family::TwoPanel));
        assert_eq!(Family::from_rated_watts(1200), Some(Family::FourPanel));
        assert_eq!(Family::from_rated_watts(1500), Some(Family::FourPanel));
    }

    #[test]
    fn unknown_rating_is_rejected() {
        assert_eq!(Family::from_rated_watts(0), None);
        assert_eq!(Family::from_rated_watts(550), None);
        assert_eq!(Family::from_rated_watts(2000), None);
    }

    #[test]
    fn describe_is_jargon_free() {
        for fam in [Family::OnePanel, Family::TwoPanel, Family::FourPanel] {
            let d = fam.describe();
            assert!(!d.is_empty());
            assert!(!d.contains("HM-"));
        }
    }
}
