//! Climate modes, the current activity, fan modes and presets.
//!
//! These mirror the Home Assistant `climate` entity-domain vocabulary
//! (`HVACMode`, `HVACAction`, fan modes, preset modes) so a vendor adapter can
//! map its device onto a model the rest of cave-home already understands. The
//! semantics are taken from the public HA climate-domain docs (Apache-2.0); no
//! source was ported.

/// What the user has *asked* the climate device to do.
///
/// This is the requested operating mode — distinct from [`HvacAction`], which
/// is what the device is actually doing right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HvacMode {
    /// The device is switched off.
    Off,
    /// Heat to a single target temperature.
    Heat,
    /// Cool to a single target temperature.
    Cool,
    /// Maintain a comfort band: heat below the low setpoint, cool above the
    /// high setpoint (HA `heat_cool`).
    HeatCool,
    /// Device decides heat vs. cool around a single target (HA `auto`).
    Auto,
    /// Dehumidify to a single target.
    Dry,
    /// Run the fan only — move air without heating or cooling.
    FanOnly,
}

impl HvacMode {
    /// Every mode, for exhaustive iteration in tests and capability listings.
    pub const ALL: [Self; 7] = [
        Self::Off,
        Self::Heat,
        Self::Cool,
        Self::HeatCool,
        Self::Auto,
        Self::Dry,
        Self::FanOnly,
    ];

    /// Whether this mode is driven by a single `target_temperature` (as opposed
    /// to a low/high band).
    #[must_use]
    pub const fn uses_single_target(self) -> bool {
        matches!(self, Self::Heat | Self::Cool | Self::Dry | Self::Auto)
    }

    /// Whether this mode is driven by a `target_temp_low` / `target_temp_high`
    /// band.
    #[must_use]
    pub const fn uses_target_band(self) -> bool {
        matches!(self, Self::HeatCool)
    }
}

/// What the device is *actually doing* right now (HA `hvac_action`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HvacAction {
    /// Switched off.
    Off,
    /// On, but neither heating nor cooling — the room is within tolerance.
    Idle,
    /// Actively heating.
    Heating,
    /// Actively cooling.
    Cooling,
    /// Actively dehumidifying.
    Drying,
    /// Running the fan only.
    Fan,
    /// Warming up to be ready to heat (e.g. a heat pump priming its loop).
    Preheating,
    /// Melting frost off an outdoor coil before it can heat again.
    Defrosting,
}

/// Fan speed selection (HA fan modes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FanMode {
    /// The device picks the speed.
    Auto,
    /// Fan runs continuously.
    On,
    Low,
    Medium,
    High,
}

impl FanMode {
    /// Every fan mode, for capability listings and tests.
    pub const ALL: [Self; 5] = [Self::Auto, Self::On, Self::Low, Self::Medium, Self::High];
}

/// Comfort presets (HA preset modes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PresetMode {
    /// No preset — follow the explicit setpoint.
    None,
    /// Energy-saving mode.
    Eco,
    /// Nobody home — let the temperature drift to a safe holding point.
    Away,
    /// Maximum output for a short, quick correction.
    Boost,
    /// Active comfort.
    Comfort,
    /// At home, normal comfort.
    Home,
    /// Quiet, gentle setpoint for the night.
    Sleep,
    /// Higher-activity setpoint (e.g. a workout room).
    Activity,
}

impl PresetMode {
    /// Every preset, for capability listings and tests.
    pub const ALL: [Self; 8] = [
        Self::None,
        Self::Eco,
        Self::Away,
        Self::Boost,
        Self::Comfort,
        Self::Home,
        Self::Sleep,
        Self::Activity,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_target_and_band_modes_are_disjoint() {
        for m in HvacMode::ALL {
            assert!(
                !(m.uses_single_target() && m.uses_target_band()),
                "{m:?} cannot use both a single target and a band"
            );
        }
    }

    #[test]
    fn heatcool_uses_band() {
        assert!(HvacMode::HeatCool.uses_target_band());
        assert!(!HvacMode::HeatCool.uses_single_target());
    }

    #[test]
    fn heat_cool_dry_auto_use_single_target() {
        assert!(HvacMode::Heat.uses_single_target());
        assert!(HvacMode::Cool.uses_single_target());
        assert!(HvacMode::Dry.uses_single_target());
        assert!(HvacMode::Auto.uses_single_target());
    }

    #[test]
    fn off_and_fan_only_use_neither_setpoint() {
        for m in [HvacMode::Off, HvacMode::FanOnly] {
            assert!(!m.uses_single_target());
            assert!(!m.uses_target_band());
        }
    }

    #[test]
    fn enum_inventories_are_complete() {
        assert_eq!(HvacMode::ALL.len(), 7);
        assert_eq!(FanMode::ALL.len(), 5);
        assert_eq!(PresetMode::ALL.len(), 8);
    }
}
