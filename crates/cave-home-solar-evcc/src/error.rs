//! Validation errors for the charge-control engine.
//!
//! The engine never panics on bad input: impossible numbers (negative power,
//! a non-finite reading, a battery that holds more than 100%, a charger wired
//! for anything other than 1 or 3 phases) are surfaced as a typed error the
//! caller can report or ignore.

/// Why a charge-control input could not be accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvccError {
    /// A watt / amp / voltage value was `NaN` or infinite.
    NotFinite,
    /// A power figure that must be non-negative (production, consumption,
    /// charge power) was below zero.
    NegativePower,
    /// A charger or charge limit was specified with a negative current.
    NegativeCurrent,
    /// The minimum current was greater than the maximum current.
    CurrentRangeInverted,
    /// Grid voltage was zero or negative.
    NonPositiveVoltage,
    /// A phase count other than 1 or 3 was requested.
    UnsupportedPhases,
    /// A battery capacity was zero or negative.
    NonPositiveCapacity,
    /// A state-of-charge percentage was outside `0..=100`.
    SocOutOfRange,
    /// A deadline (hours remaining) was negative.
    NegativeDeadline,
}

impl core::fmt::Display for EvccError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let msg = match self {
            Self::NotFinite => "a value is not a finite number",
            Self::NegativePower => "a power value is negative",
            Self::NegativeCurrent => "a current value is negative",
            Self::CurrentRangeInverted => "minimum current exceeds maximum current",
            Self::NonPositiveVoltage => "grid voltage must be greater than zero",
            Self::UnsupportedPhases => "charger phase count must be 1 or 3",
            Self::NonPositiveCapacity => "battery capacity must be greater than zero",
            Self::SocOutOfRange => "state of charge must be between 0 and 100",
            Self::NegativeDeadline => "hours remaining cannot be negative",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for EvccError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_render_human_readable() {
        assert!(EvccError::UnsupportedPhases.to_string().contains("1 or 3"));
        assert!(EvccError::SocOutOfRange.to_string().contains("0 and 100"));
    }

    #[test]
    fn errors_are_distinct() {
        assert_ne!(EvccError::NotFinite, EvccError::NegativePower);
    }
}
