//! Charge modes — the four ways cave-home will charge your car.
//!
//! These mirror the well-documented evcc-class charging semantics, named in
//! household language. The mode decides *whether* and *from where* energy
//! flows to the car; the rest of the engine ([`crate::current`],
//! [`crate::phase`], [`crate::antiflap`]) decides *how much* and *when*.

/// How cave-home should charge your car right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChargeMode {
    /// Don't charge at all.
    Off,
    /// Charge as fast as possible from whatever is available, pulling from the
    /// grid if the sun isn't enough ("I need the car ready, now").
    Now,
    /// Always guarantee a minimum charge (topping up from the grid when the
    /// sun is short) and use any extra sunshine on top.
    MinPlusPv,
    /// Only charge from spare sunshine; pause entirely when there isn't enough.
    PvOnly,
}

impl ChargeMode {
    /// Whether this mode is willing to draw from the grid when the sun is short.
    ///
    /// `PvOnly` never does; `Now` and `MinPlusPv` do; `Off` charges from
    /// nowhere.
    #[must_use]
    pub const fn allows_grid(self) -> bool {
        matches!(self, Self::Now | Self::MinPlusPv)
    }

    /// Whether this mode charges at all.
    #[must_use]
    pub const fn is_charging_intent(self) -> bool {
        !matches!(self, Self::Off)
    }

    /// Whether this mode uses spare sunshine on top of any minimum.
    #[must_use]
    pub const fn uses_surplus(self) -> bool {
        matches!(self, Self::PvOnly | Self::MinPlusPv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_never_charges() {
        assert!(!ChargeMode::Off.is_charging_intent());
        assert!(!ChargeMode::Off.allows_grid());
        assert!(!ChargeMode::Off.uses_surplus());
    }

    #[test]
    fn now_pulls_grid_and_charges() {
        assert!(ChargeMode::Now.is_charging_intent());
        assert!(ChargeMode::Now.allows_grid());
    }

    #[test]
    fn pv_only_never_pulls_grid() {
        assert!(ChargeMode::PvOnly.is_charging_intent());
        assert!(!ChargeMode::PvOnly.allows_grid());
        assert!(ChargeMode::PvOnly.uses_surplus());
    }

    #[test]
    fn min_plus_pv_does_both() {
        assert!(ChargeMode::MinPlusPv.allows_grid());
        assert!(ChargeMode::MinPlusPv.uses_surplus());
    }
}
