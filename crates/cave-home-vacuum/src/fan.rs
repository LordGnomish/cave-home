//! Suction-power (fan-speed) model with capability gating.
//!
//! Vacuums differ in how many suction steps they expose: a basic unit may only
//! offer Low / Medium / High, while a flagship adds Min, Max and a Turbo boost.
//! Valetudo and the HA `vacuum` domain both model fan speed as a named preset
//! rather than a raw percentage, so cave-home does too — the household picks
//! "quiet" or "strong", never a number.

/// A suction-power preset, ordered weakest → strongest.
///
/// `Off` means the fan is not running (used for mopping-only passes on units
/// that support it); the cleaning presets run from `Min` up to `Turbo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FanSpeed {
    /// Fan off (e.g. a mop-only pass).
    Off,
    /// Lowest suction the unit offers.
    Min,
    Low,
    Medium,
    High,
    /// Strongest steady suction.
    Max,
    /// A short, extra-strong boost above `Max` (deep-clean preset).
    Turbo,
}

impl FanSpeed {
    /// All presets, weakest → strongest. Handy for UI pickers and tests.
    pub const ALL: [Self; 7] = [
        Self::Off,
        Self::Min,
        Self::Low,
        Self::Medium,
        Self::High,
        Self::Max,
        Self::Turbo,
    ];
}

/// What suction presets a particular vacuum can actually do.
///
/// The set is described by the strongest preset the hardware supports and
/// whether it can turn the fan fully off. A request for an unsupported preset is
/// rejected by [`FanCapability::supports`] rather than silently clamped, so the
/// household is told the unit cannot do that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FanCapability {
    /// The strongest preset this vacuum can run.
    max: FanSpeed,
    /// Whether the vacuum can run with the fan fully off (mop-only).
    can_turn_off: bool,
}

impl FanCapability {
    /// A modest three-step vacuum: Low / Medium / High, no off, no turbo.
    #[must_use]
    pub const fn basic() -> Self {
        Self { max: FanSpeed::High, can_turn_off: false }
    }

    /// A full-range vacuum: everything from a mop-only off up to Turbo.
    #[must_use]
    pub const fn full() -> Self {
        Self { max: FanSpeed::Turbo, can_turn_off: true }
    }

    /// Build a capability from an explicit ceiling and off-support flag.
    #[must_use]
    pub const fn new(max: FanSpeed, can_turn_off: bool) -> Self {
        Self { max, can_turn_off }
    }

    /// The strongest preset this vacuum supports.
    #[must_use]
    pub const fn max(self) -> FanSpeed {
        self.max
    }

    /// Whether this vacuum can run with the fan fully off.
    #[must_use]
    pub const fn can_turn_off(self) -> bool {
        self.can_turn_off
    }

    /// Whether `speed` is one this vacuum can actually run.
    #[must_use]
    pub fn supports(self, speed: FanSpeed) -> bool {
        match speed {
            FanSpeed::Off => self.can_turn_off,
            other => other <= self.max,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_order_weakest_to_strongest() {
        assert!(FanSpeed::Off < FanSpeed::Min);
        assert!(FanSpeed::Min < FanSpeed::Low);
        assert!(FanSpeed::Low < FanSpeed::Medium);
        assert!(FanSpeed::Medium < FanSpeed::High);
        assert!(FanSpeed::High < FanSpeed::Max);
        assert!(FanSpeed::Max < FanSpeed::Turbo);
    }

    #[test]
    fn basic_unit_gates_turbo_and_off() {
        let cap = FanCapability::basic();
        assert!(cap.supports(FanSpeed::Low));
        assert!(cap.supports(FanSpeed::Medium));
        assert!(cap.supports(FanSpeed::High));
        assert!(!cap.supports(FanSpeed::Max), "basic unit cannot reach Max");
        assert!(!cap.supports(FanSpeed::Turbo), "basic unit cannot Turbo");
        assert!(!cap.supports(FanSpeed::Off), "basic unit cannot mop-only");
    }

    #[test]
    fn full_unit_supports_everything() {
        let cap = FanCapability::full();
        for speed in FanSpeed::ALL {
            assert!(cap.supports(speed), "full unit must support {speed:?}");
        }
    }

    #[test]
    fn custom_ceiling_gates_above_it() {
        let cap = FanCapability::new(FanSpeed::Medium, false);
        assert!(cap.supports(FanSpeed::Medium));
        assert!(!cap.supports(FanSpeed::High));
        assert!(!cap.supports(FanSpeed::Off));
    }
}
