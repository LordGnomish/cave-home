//! Error-code model: the faults a robot vacuum can report, each with a
//! grandma-friendly explanation.
//!
//! Valetudo and the HA `vacuum` domain surface a numeric/string error code from
//! the vacuum. cave-home maps those onto a small, named [`ErrorCode`] set and —
//! per Charter §6.3 / ADR-007 — turns each into plain advice a household can act
//! on ("free its brush", "empty the dustbin"), never a raw code or vendor term.
//!
//! Localised text lives in [`crate::label`]; this module owns the fault taxonomy
//! and which faults a person can clear themselves.

/// A fault a robot vacuum can report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// The main brush is tangled or jammed (hair, a sock).
    BrushStuck,
    /// A drive wheel is stuck or hanging (caught on a ledge / threshold).
    WheelStuck,
    /// The side brush is tangled.
    SideBrushStuck,
    /// The dustbin is full and needs emptying.
    BinFull,
    /// The dustbin is not seated / missing.
    DustbinMissing,
    /// The vacuum cannot work out where it is on its map.
    Lost,
    /// The vacuum is wedged and cannot move (under furniture, in a corner).
    Trapped,
    /// A cliff / drop sensor tripped (top of a stair, a step down).
    CliffSensor,
    /// The water/mop tank is empty (for mopping units).
    WaterTankEmpty,
    /// A general fault the vacuum could not categorise.
    Generic,
}

impl ErrorCode {
    /// All known faults. Handy for exhaustiveness tests and UI listings.
    pub const ALL: [Self; 10] = [
        Self::BrushStuck,
        Self::WheelStuck,
        Self::SideBrushStuck,
        Self::BinFull,
        Self::DustbinMissing,
        Self::Lost,
        Self::Trapped,
        Self::CliffSensor,
        Self::WaterTankEmpty,
        Self::Generic,
    ];

    /// Whether a person can typically clear this fault themselves on the spot
    /// (free a brush, empty the bin) versus a fault that needs the vacuum to be
    /// moved / rescued first. Both still require a human; this only shades the
    /// advice tone.
    #[must_use]
    pub const fn is_user_serviceable(self) -> bool {
        matches!(
            self,
            Self::BrushStuck
                | Self::SideBrushStuck
                | Self::BinFull
                | Self::DustbinMissing
                | Self::WaterTankEmpty
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_serviceable_split_is_sensible() {
        assert!(ErrorCode::BrushStuck.is_user_serviceable());
        assert!(ErrorCode::BinFull.is_user_serviceable());
        assert!(!ErrorCode::Lost.is_user_serviceable());
        assert!(!ErrorCode::Trapped.is_user_serviceable());
        assert!(!ErrorCode::CliffSensor.is_user_serviceable());
    }

    #[test]
    fn all_lists_every_variant_once() {
        // A light guard that ALL stays in sync as variants are added.
        assert_eq!(ErrorCode::ALL.len(), 10);
    }
}
