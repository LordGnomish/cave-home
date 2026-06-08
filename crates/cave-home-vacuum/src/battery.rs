//! Battery model: a validated charge percentage, charging direction, and the
//! low-battery auto-return threshold logic.
//!
//! The HA `vacuum` domain surfaces a battery level and a charging flag; Valetudo
//! reports the same. cave-home turns those into a small value type that the
//! state machine consults to decide when a cleaning vacuum must abandon the job
//! and drive home before it strands itself on the floor.

/// Whether the battery is gaining or losing charge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChargeDirection {
    /// On the dock, taking on charge.
    Charging,
    /// Off the dock, running the battery down.
    Discharging,
}

/// Why a [`Battery`] could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryError {
    /// The percentage was outside the valid `0..=100` range.
    OutOfRange,
}

impl core::fmt::Display for BatteryError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::OutOfRange => f.write_str("battery percentage must be between 0 and 100"),
        }
    }
}

impl std::error::Error for BatteryError {}

/// The default charge below which a cleaning vacuum should head home. Chosen so
/// there is enough reserve to actually reach the dock from across a home.
pub const DEFAULT_RETURN_THRESHOLD: u8 = 20;

/// A robot battery: a validated 0..=100 percentage plus its charge direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Battery {
    percent: u8,
    direction: ChargeDirection,
    /// The percentage at or below which a busy vacuum should auto-return.
    return_threshold: u8,
}

impl Battery {
    /// Construct a battery with the default auto-return threshold.
    ///
    /// # Errors
    /// [`BatteryError::OutOfRange`] if `percent` is above 100.
    pub const fn new(percent: u8, direction: ChargeDirection) -> Result<Self, BatteryError> {
        Self::with_threshold(percent, direction, DEFAULT_RETURN_THRESHOLD)
    }

    /// Construct a battery with an explicit auto-return threshold.
    ///
    /// # Errors
    /// [`BatteryError::OutOfRange`] if `percent` or `return_threshold` exceeds
    /// 100.
    pub const fn with_threshold(
        percent: u8,
        direction: ChargeDirection,
        return_threshold: u8,
    ) -> Result<Self, BatteryError> {
        if percent > 100 || return_threshold > 100 {
            return Err(BatteryError::OutOfRange);
        }
        Ok(Self { percent, direction, return_threshold })
    }

    #[must_use]
    pub const fn percent(self) -> u8 {
        self.percent
    }

    #[must_use]
    pub const fn direction(self) -> ChargeDirection {
        self.direction
    }

    #[must_use]
    pub const fn return_threshold(self) -> u8 {
        self.return_threshold
    }

    #[must_use]
    pub const fn is_charging(self) -> bool {
        matches!(self.direction, ChargeDirection::Charging)
    }

    /// Whether the charge has fallen to (or below) the auto-return threshold.
    /// A vacuum that is already charging never reports low — it is on the dock.
    #[must_use]
    pub const fn is_low(self) -> bool {
        !self.is_charging() && self.percent <= self.return_threshold
    }

    /// Whether the battery is full enough to start a fresh clean. A vacuum
    /// sitting at or below the return threshold should top up first.
    #[must_use]
    pub const fn can_start_clean(self) -> bool {
        self.percent > self.return_threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_over_one_hundred() {
        assert_eq!(
            Battery::new(101, ChargeDirection::Discharging),
            Err(BatteryError::OutOfRange)
        );
    }

    #[test]
    fn rejects_over_range_threshold() {
        assert_eq!(
            Battery::with_threshold(50, ChargeDirection::Discharging, 200),
            Err(BatteryError::OutOfRange)
        );
    }

    #[test]
    fn accepts_full_range() {
        assert!(Battery::new(0, ChargeDirection::Discharging).is_ok());
        assert!(Battery::new(100, ChargeDirection::Charging).is_ok());
    }

    #[test]
    fn discharging_below_threshold_is_low() {
        let b = Battery::new(15, ChargeDirection::Discharging).expect("valid");
        assert!(b.is_low());
        assert!(!b.can_start_clean());
    }

    #[test]
    fn at_threshold_is_low() {
        let b = Battery::new(DEFAULT_RETURN_THRESHOLD, ChargeDirection::Discharging)
            .expect("valid");
        assert!(b.is_low(), "at the threshold counts as low");
    }

    #[test]
    fn charging_is_never_low() {
        // Even at 5% on the dock, charging means "do not panic-return".
        let b = Battery::new(5, ChargeDirection::Charging).expect("valid");
        assert!(!b.is_low());
        assert!(b.is_charging());
    }

    #[test]
    fn well_charged_can_start() {
        let b = Battery::new(80, ChargeDirection::Discharging).expect("valid");
        assert!(!b.is_low());
        assert!(b.can_start_clean());
    }
}
