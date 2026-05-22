// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: evcc-io/evcc@7303a5b476be7fa3da35807df899651f47b3d2f0 core/loadpoint.go,
//         core/loadpoint_phases.go, core/loadpoint_charger.go,
//         core/loadpoint_smartcost.go, core/loadpoint_vehicle.go.
//
//! Loadpoint — a single controllable load (EV wallbox or heat-pump).
//!
//! In the upstream this is the giant `Loadpoint` struct that bundles
//! charger I/O, vehicle binding, planner state, and surplus loop
//! decisions. We split that into:
//!
//! * Pure value types in this module ([`Loadpoint`], [`MinMaxCurrent`],
//!   [`PhaseCount`], [`ChargeMode`], [`LoadpointStatus`], [`Kind`]).
//! * The decision step ([`Loadpoint::next_decision`]) which is the
//!   distilled `loadpoint.Update()` core: given the present surplus and
//!   tariff snapshot, decide the next charge current.
//!
//! Charter §6.3 grandma-friendly UX: this module exposes `EvCharger`
//! and `HeatPump` kinds — never "loadpoint" in user-facing strings.
//! The struct name is preserved for upstream-parity readability.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

/// Charge mode — direct port of the upstream `api.ChargeMode` enum.
///
/// Source: `evcc-io/evcc@7303a5b…/api/types.go`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChargeMode {
    /// "Off" — disable charging entirely.
    Off,
    /// "Now" — charge at the configured `MaxCurrent`, ignoring surplus.
    Now,
    /// "MinPV" — always charge at `MinCurrent`, top-up from surplus.
    MinPv,
    /// "PV" — only charge from PV surplus, idle otherwise.
    Pv,
}

impl ChargeMode {
    /// Stable string used in the Portal and `cavehomectl`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Now => "now",
            Self::MinPv => "minpv",
            Self::Pv => "pv",
        }
    }

    /// Parse from the upstream string vocabulary.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "off" => Some(Self::Off),
            "now" => Some(Self::Now),
            "minpv" => Some(Self::MinPv),
            "pv" => Some(Self::Pv),
            _ => None,
        }
    }
}

/// Loadpoint kind — distinguishes an EV charger from a heat-pump.
///
/// Upstream EVCC treats these uniformly via the "Charger" interface;
/// cave-home tags them so the Portal can render grandma-friendly icons
/// and the planner can apply heat-pump-specific minimum-runtime rules
/// (Charter §6.3, ADR-012).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    EvCharger,
    HeatPump,
}

/// Phase configuration. Upstream `core/loadpoint_phases.go` encodes
/// this as plain `int` (1, 2, 3) plus a `phasesConfigured` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhaseCount {
    Single,
    Two,
    Three,
}

impl PhaseCount {
    #[must_use]
    pub const fn as_int(self) -> u8 {
        match self {
            Self::Single => 1,
            Self::Two => 2,
            Self::Three => 3,
        }
    }

    #[must_use]
    pub const fn from_int(n: u8) -> Option<Self> {
        match n {
            1 => Some(Self::Single),
            2 => Some(Self::Two),
            3 => Some(Self::Three),
            _ => None,
        }
    }
}

/// Current envelope. Upstream: `Loadpoint.MinCurrent` / `MaxCurrent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MinMaxCurrent {
    pub min_a: u16,
    pub max_a: u16,
}

impl MinMaxCurrent {
    /// EVCC default 6 A floor, 16 A ceiling — matches upstream
    /// `core/loadpoint.go::NewLoadpoint`.
    #[must_use]
    pub const fn default_iec_wallbox() -> Self {
        Self { min_a: 6, max_a: 16 }
    }

    /// Returns `Err` if `requested` falls outside `[min, max]`.
    pub fn clamp_or_err(self, name: &str, requested: u16) -> Result<u16> {
        if requested < self.min_a || requested > self.max_a {
            Err(Error::CurrentOutOfRange {
                name: name.to_string(),
                requested_a: requested,
                min_a: self.min_a,
                max_a: self.max_a,
            })
        } else {
            Ok(requested)
        }
    }
}

/// Loadpoint status — direct port of upstream `api.ChargeStatus`.
///
/// Source: `evcc-io/evcc@7303a5b…/api/types.go`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoadpointStatus {
    /// Plugged out (`A`).
    Disconnected,
    /// Plugged in, not charging (`B`).
    Connected,
    /// Charging (`C`).
    Charging,
    /// Charging with ventilation required (`D`).
    ChargingVentilated,
    /// Error (`E` / `F`).
    Errored,
}

impl LoadpointStatus {
    /// Returns the upstream single-letter glyph (`A..F`).
    #[must_use]
    pub const fn glyph(self) -> char {
        match self {
            Self::Disconnected => 'A',
            Self::Connected => 'B',
            Self::Charging => 'C',
            Self::ChargingVentilated => 'D',
            Self::Errored => 'E',
        }
    }
}

/// Loadpoint snapshot. Equivalent to a flattened
/// `Loadpoint{charger, phases, MinCurrent, MaxCurrent, mode, vehicle}`
/// from upstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Loadpoint {
    pub name: String,
    pub kind: Kind,
    pub mode: ChargeMode,
    pub phases: PhaseCount,
    pub current_envelope: MinMaxCurrent,
    pub status: LoadpointStatus,
    /// Present charging current in amps. `None` if charger hasn't
    /// reported a sample yet.
    pub charge_current_a: Option<u16>,
    /// Vehicle / load SoC if known.
    pub soc_percent: Option<u8>,
    /// Minimum-on-time floor in seconds, primarily for `HeatPump`.
    pub min_on_time_s: u32,
    /// Whether the loadpoint can switch its phase count at runtime.
    pub phase_switching: bool,
}

impl Loadpoint {
    /// Construct a 16 A / 3-phase IEC wallbox with PV mode. Mirrors
    /// upstream `NewLoadpoint` defaults.
    #[must_use]
    pub fn new_ev(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: Kind::EvCharger,
            mode: ChargeMode::Pv,
            phases: PhaseCount::Three,
            current_envelope: MinMaxCurrent::default_iec_wallbox(),
            status: LoadpointStatus::Disconnected,
            charge_current_a: None,
            soc_percent: None,
            min_on_time_s: 0,
            phase_switching: true,
        }
    }

    /// Construct a heat-pump loadpoint. Heat-pumps don't switch phases
    /// at runtime, must run for at least 10 minutes once started, and
    /// default to `MinPv` so they still draw the resistor-heater
    /// minimum overnight.
    #[must_use]
    pub fn new_heat_pump(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: Kind::HeatPump,
            mode: ChargeMode::MinPv,
            phases: PhaseCount::Three,
            current_envelope: MinMaxCurrent::default_iec_wallbox(),
            status: LoadpointStatus::Connected,
            charge_current_a: None,
            soc_percent: None,
            min_on_time_s: 600,
            phase_switching: false,
        }
    }

    /// Set the charge mode.
    pub fn set_mode(&mut self, mode: ChargeMode) {
        self.mode = mode;
        if matches!(mode, ChargeMode::Off) {
            self.charge_current_a = Some(0);
        }
    }

    /// Switch phases. Returns `Err` for chargers that don't support it.
    pub fn set_phases(&mut self, phases: PhaseCount) -> Result<()> {
        if !self.phase_switching && phases != self.phases {
            return Err(Error::PhaseSwitchUnsupported(self.name.clone()));
        }
        self.phases = phases;
        Ok(())
    }

    /// Set the achievable charge current (clamped to the envelope).
    pub fn set_charge_current(&mut self, requested_a: u16) -> Result<()> {
        let clamped = if requested_a == 0 {
            0
        } else {
            self.current_envelope.clamp_or_err(&self.name, requested_a)?
        };
        self.charge_current_a = Some(clamped);
        Ok(())
    }

    /// Convert an envelope current value to power in watts for the
    /// current phase count. Uses the European 230 V nominal.
    ///
    /// Source: upstream `core/loadpoint.go::chargePower`.
    #[must_use]
    pub const fn current_to_watts(amps: u16, phases: PhaseCount) -> u32 {
        // 230 V nominal — upstream uses `Voltage` constant 230.
        230u32 * amps as u32 * phases.as_int() as u32
    }

    /// Convert available surplus power in watts to a charge current
    /// (clamped against the envelope and phase count). Returns the
    /// largest current that fits inside `surplus_w`.
    ///
    /// Source: upstream `core/loadpoint.go::pvMaxCurrent`.
    #[must_use]
    pub fn watts_to_current(&self, surplus_w: i32) -> u16 {
        if surplus_w <= 0 {
            return 0;
        }
        let phases = self.phases.as_int() as i32;
        // u16-safe floor — i32 surplus is bounded to ~megawatts.
        let a = surplus_w / (230 * phases);
        if a < self.current_envelope.min_a as i32 {
            0
        } else if a > self.current_envelope.max_a as i32 {
            self.current_envelope.max_a
        } else {
            a as u16
        }
    }

    /// One full decision step. Mirrors upstream `Loadpoint.Update`:
    /// pick the next charge current given (a) the current charge mode
    /// and (b) the surplus power offered by `Site`.
    ///
    /// Returns the **next charge current in amps** (0 means "stop
    /// charging this tick").
    #[must_use]
    pub fn next_decision(&self, surplus_w: i32) -> u16 {
        match self.mode {
            ChargeMode::Off => 0,
            ChargeMode::Now => self.current_envelope.max_a,
            ChargeMode::MinPv => {
                let pv = self.watts_to_current(surplus_w);
                pv.max(self.current_envelope.min_a)
            }
            ChargeMode::Pv => self.watts_to_current(surplus_w),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charge_mode_round_trip() {
        for mode in [ChargeMode::Off, ChargeMode::Now, ChargeMode::MinPv, ChargeMode::Pv] {
            assert_eq!(ChargeMode::parse(mode.as_str()), Some(mode));
        }
    }

    #[test]
    fn charge_mode_parse_rejects_unknown() {
        assert_eq!(ChargeMode::parse("yolo"), None);
    }

    #[test]
    fn phase_count_round_trip() {
        for n in 1..=3 {
            assert_eq!(PhaseCount::from_int(n).map(PhaseCount::as_int), Some(n));
        }
        assert!(PhaseCount::from_int(4).is_none());
    }

    #[test]
    fn current_envelope_clamps() {
        let env = MinMaxCurrent::default_iec_wallbox();
        assert_eq!(env.clamp_or_err("lp1", 10).ok(), Some(10));
        assert!(env.clamp_or_err("lp1", 5).is_err());
        assert!(env.clamp_or_err("lp1", 32).is_err());
    }

    #[test]
    fn watts_to_current_three_phase_16a() {
        let lp = Loadpoint::new_ev("lp1");
        // 230 V × 3 phases × 16 A = 11.04 kW
        assert_eq!(lp.watts_to_current(11_040), 16);
        assert_eq!(lp.watts_to_current(11_500), 16); // ceiling clamp
        assert_eq!(lp.watts_to_current(2_000), 0); // under min
        assert_eq!(lp.watts_to_current(-500), 0); // import, no surplus
    }

    #[test]
    fn watts_to_current_single_phase_floor() {
        let mut lp = Loadpoint::new_ev("lp1");
        lp.set_phases(PhaseCount::Single).unwrap();
        // 230 V × 1 × 6 A = 1380 W min
        assert_eq!(lp.watts_to_current(1_500), 6);
        assert_eq!(lp.watts_to_current(1_300), 0);
    }

    #[test]
    fn current_to_watts_three_phase() {
        assert_eq!(Loadpoint::current_to_watts(16, PhaseCount::Three), 11_040);
        assert_eq!(Loadpoint::current_to_watts(6, PhaseCount::Single), 1_380);
    }

    #[test]
    fn set_charge_current_clamped() {
        let mut lp = Loadpoint::new_ev("lp1");
        assert!(lp.set_charge_current(10).is_ok());
        assert_eq!(lp.charge_current_a, Some(10));
        assert!(lp.set_charge_current(32).is_err());
        assert!(lp.set_charge_current(0).is_ok());
        assert_eq!(lp.charge_current_a, Some(0));
    }

    #[test]
    fn set_phases_rejected_on_fixed_charger() {
        let mut lp = Loadpoint::new_heat_pump("hp1");
        assert!(lp.set_phases(PhaseCount::Single).is_err());
    }

    #[test]
    fn off_mode_sets_zero_current() {
        let mut lp = Loadpoint::new_ev("lp1");
        lp.set_mode(ChargeMode::Off);
        assert_eq!(lp.charge_current_a, Some(0));
        assert_eq!(lp.next_decision(20_000), 0);
    }

    #[test]
    fn now_mode_ignores_surplus() {
        let lp = Loadpoint {
            mode: ChargeMode::Now,
            ..Loadpoint::new_ev("lp1")
        };
        assert_eq!(lp.next_decision(0), 16);
        assert_eq!(lp.next_decision(-5_000), 16);
    }

    #[test]
    fn minpv_mode_floors_to_min_a() {
        let lp = Loadpoint::new_ev("lp1");
        // mode is Pv by default; force MinPv
        let lp = Loadpoint {
            mode: ChargeMode::MinPv,
            ..lp
        };
        assert_eq!(lp.next_decision(0), lp.current_envelope.min_a);
        assert_eq!(lp.next_decision(11_500), 16);
    }

    #[test]
    fn loadpoint_status_glyphs_match_upstream() {
        assert_eq!(LoadpointStatus::Disconnected.glyph(), 'A');
        assert_eq!(LoadpointStatus::Charging.glyph(), 'C');
    }

    #[test]
    fn heat_pump_defaults_set_min_on_time() {
        let lp = Loadpoint::new_heat_pump("hp1");
        assert_eq!(lp.kind, Kind::HeatPump);
        assert_eq!(lp.min_on_time_s, 600);
        assert!(!lp.phase_switching);
    }
}
