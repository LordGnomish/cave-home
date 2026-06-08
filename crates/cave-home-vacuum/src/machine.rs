//! The vacuum state machine: apply commands, settle movement events, react to
//! battery, surface and gate faults.
//!
//! Modelled on the Home Assistant `vacuum` entity domain and Valetudo's control
//! surface (both permissive — ADR-017): a command is accepted only when it makes
//! sense for the current state and the vacuum's capabilities; a fault drives the
//! vacuum into [`VacuumState::Error`] and **gates** further cleaning commands
//! until a human clears it; and a cleaning vacuum that runs low on battery
//! abandons the job and drives home on its own.

use crate::battery::Battery;
use crate::error::ErrorCode;
use crate::fan::{FanCapability, FanSpeed};
use crate::map::{SegmentRequestError, VacuumMap, Zone};
use crate::state::{VacuumCommand, VacuumState};

/// What a particular vacuum can do, beyond moving and cleaning the whole floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VacuumFeatures {
    /// The suction presets this unit supports.
    pub fan: FanCapability,
    /// Whether the unit can clean a single spot ([`VacuumCommand::CleanSpot`]).
    pub supports_spot: bool,
    /// Whether the unit can clean named rooms / arbitrary zones (needs a saved
    /// map). Maps to Valetudo's segment/zone cleaning capability.
    pub supports_zones: bool,
}

impl VacuumFeatures {
    /// A basic vacuum: three suction steps, spot clean, no map-based cleaning.
    #[must_use]
    pub const fn basic() -> Self {
        Self {
            fan: FanCapability::basic(),
            supports_spot: true,
            supports_zones: false,
        }
    }

    /// A full-featured, map-aware vacuum: every suction preset, spot clean, and
    /// room/zone cleaning.
    #[must_use]
    pub const fn full() -> Self {
        Self {
            fan: FanCapability::full(),
            supports_spot: true,
            supports_zones: true,
        }
    }
}

/// Why a command was rejected by the machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    /// The vacuum does not advertise the capability this command needs (e.g.
    /// room cleaning on a map-less unit, or a suction preset it cannot run).
    Unsupported,
    /// The command makes no sense from the current state (e.g. pausing a
    /// vacuum that is already docked).
    IllegalTransition,
    /// The vacuum is in a fault state and this command is gated until the fault
    /// is cleared.
    Faulted,
    /// The battery is too low to begin a fresh clean.
    BatteryTooLow,
    /// A room/zone clean request did not validate against the saved map.
    BadCleanRequest(SegmentRequestError),
}

impl core::fmt::Display for CommandError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unsupported => f.write_str("the vacuum cannot do that"),
            Self::IllegalTransition => f.write_str("the vacuum cannot do that right now"),
            Self::Faulted => f.write_str("the vacuum needs attention before it can do that"),
            Self::BatteryTooLow => f.write_str("the battery is too low to start cleaning"),
            Self::BadCleanRequest(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CommandError {}

/// A robot vacuum: its current state, battery, capabilities, chosen suction,
/// saved map, and any active fault.
#[derive(Debug, Clone)]
pub struct Vacuum {
    state: VacuumState,
    battery: Battery,
    features: VacuumFeatures,
    fan_speed: FanSpeed,
    map: VacuumMap,
    error: Option<ErrorCode>,
}

impl Vacuum {
    /// A vacuum with a known starting state, an empty map and a Medium suction
    /// default.
    #[must_use]
    pub const fn with_state(features: VacuumFeatures, state: VacuumState, battery: Battery) -> Self {
        Self {
            state,
            battery,
            features,
            fan_speed: FanSpeed::Medium,
            map: VacuumMap::empty(),
            error: None,
        }
    }

    /// A vacuum resting on its dock, fully aware of its saved map.
    #[must_use]
    pub const fn docked(features: VacuumFeatures, battery: Battery, map: VacuumMap) -> Self {
        Self {
            state: VacuumState::Docked,
            battery,
            features,
            fan_speed: FanSpeed::Medium,
            map,
            error: None,
        }
    }

    #[must_use]
    pub const fn state(&self) -> VacuumState {
        self.state
    }

    #[must_use]
    pub const fn battery(&self) -> Battery {
        self.battery
    }

    #[must_use]
    pub const fn features(&self) -> VacuumFeatures {
        self.features
    }

    #[must_use]
    pub const fn fan_speed(&self) -> FanSpeed {
        self.fan_speed
    }

    #[must_use]
    pub const fn error(&self) -> Option<ErrorCode> {
        self.error
    }

    #[must_use]
    pub const fn map(&self) -> &VacuumMap {
        &self.map
    }

    /// Whether a clean may begin from the current settled state. Cleaning starts
    /// from a resting state (Idle / Docked / Paused), never mid-move.
    const fn can_begin_clean(&self) -> bool {
        matches!(
            self.state,
            VacuumState::Idle | VacuumState::Docked | VacuumState::Paused
        )
    }

    /// Apply a command. On success returns the resulting state.
    ///
    /// # Errors
    /// [`CommandError`] when the command is gated by a fault, unsupported by the
    /// hardware, illegal from the current state, blocked by a low battery, or
    /// (for room/zone cleans) fails map validation. A rejected command never
    /// changes the vacuum.
    pub fn apply(&mut self, command: VacuumCommand) -> Result<VacuumState, CommandError> {
        // Fault gate first: while faulted, only the explicitly-allowed commands
        // (Locate, ReturnToBase) get through; everything else is held.
        if self.error.is_some() && !command.allowed_in_error() {
            return Err(CommandError::Faulted);
        }

        match command {
            VacuumCommand::Locate => {
                // Locate is a no-op on state: it just makes a sound. Allowed any
                // time, including while faulted, so a person can find the unit.
                Ok(self.state)
            }
            VacuumCommand::SetFanSpeed(speed) => {
                if !self.features.fan.supports(speed) {
                    return Err(CommandError::Unsupported);
                }
                self.fan_speed = speed;
                Ok(self.state)
            }
            VacuumCommand::Start => {
                if !self.can_begin_clean() {
                    return Err(CommandError::IllegalTransition);
                }
                if !self.battery.can_start_clean() {
                    return Err(CommandError::BatteryTooLow);
                }
                self.state = VacuumState::Cleaning;
                Ok(self.state)
            }
            VacuumCommand::CleanSpot => {
                if !self.features.supports_spot {
                    return Err(CommandError::Unsupported);
                }
                if !self.can_begin_clean() {
                    return Err(CommandError::IllegalTransition);
                }
                if !self.battery.can_start_clean() {
                    return Err(CommandError::BatteryTooLow);
                }
                self.state = VacuumState::SpotCleaning;
                Ok(self.state)
            }
            VacuumCommand::CleanSegments(ids) => {
                if !self.features.supports_zones {
                    return Err(CommandError::Unsupported);
                }
                // Validate against the saved map *before* touching state.
                self.map
                    .validate_segments(&ids)
                    .map_err(CommandError::BadCleanRequest)?;
                if !self.can_begin_clean() {
                    return Err(CommandError::IllegalTransition);
                }
                if !self.battery.can_start_clean() {
                    return Err(CommandError::BatteryTooLow);
                }
                self.state = VacuumState::Cleaning;
                Ok(self.state)
            }
            VacuumCommand::CleanZones(zones) => {
                if !self.features.supports_zones {
                    return Err(CommandError::Unsupported);
                }
                if zones.is_empty() {
                    return Err(CommandError::BadCleanRequest(SegmentRequestError::Empty));
                }
                if !self.can_begin_clean() {
                    return Err(CommandError::IllegalTransition);
                }
                if !self.battery.can_start_clean() {
                    return Err(CommandError::BatteryTooLow);
                }
                self.state = VacuumState::Cleaning;
                Ok(self.state)
            }
            VacuumCommand::Pause => {
                if !self.state.is_busy() {
                    return Err(CommandError::IllegalTransition);
                }
                self.state = VacuumState::Paused;
                Ok(self.state)
            }
            VacuumCommand::Stop => {
                // Stop is meaningful from any moving or paused state; it parks
                // the vacuum where it is (Idle) rather than sending it home.
                if !matches!(
                    self.state,
                    VacuumState::Cleaning
                        | VacuumState::SpotCleaning
                        | VacuumState::Returning
                        | VacuumState::Paused
                        | VacuumState::Manual
                ) {
                    return Err(CommandError::IllegalTransition);
                }
                self.state = VacuumState::Idle;
                Ok(self.state)
            }
            VacuumCommand::ReturnToBase => {
                // Already docked? Nothing to do, but not an error.
                if matches!(self.state, VacuumState::Returning | VacuumState::Docked) {
                    return Ok(self.state);
                }
                self.state = VacuumState::Returning;
                Ok(self.state)
            }
        }
    }

    /// Convenience: clean a set of named rooms by id. Equivalent to
    /// `apply(VacuumCommand::CleanSegments(..))` but spelled out for callers.
    ///
    /// # Errors
    /// Same as [`Vacuum::apply`] for a [`VacuumCommand::CleanSegments`].
    pub fn clean_rooms(&mut self, ids: &[u16]) -> Result<VacuumState, CommandError> {
        self.apply(VacuumCommand::CleanSegments(ids.to_vec()))
    }

    /// Convenience: clean a set of zones.
    ///
    /// # Errors
    /// Same as [`Vacuum::apply`] for a [`VacuumCommand::CleanZones`].
    pub fn clean_zones(&mut self, zones: &[Zone]) -> Result<VacuumState, CommandError> {
        self.apply(VacuumCommand::CleanZones(zones.to_vec()))
    }

    /// The vacuum reports it has reached its dock: it settles to `Docked`. A
    /// docked vacuum is taking on charge, so any active *low-battery* condition
    /// is implicitly resolving — but a real fault must still be cleared
    /// explicitly via [`Vacuum::clear_error`].
    pub const fn reached_dock(&mut self) {
        if self.error.is_none() {
            self.state = VacuumState::Docked;
        }
    }

    /// The vacuum reports a fault: it enters [`VacuumState::Error`] and records
    /// the code. Cleaning commands are gated until [`Vacuum::clear_error`].
    pub const fn report_error(&mut self, code: ErrorCode) {
        self.error = Some(code);
        self.state = VacuumState::Error;
    }

    /// A human cleared the fault (freed the brush, emptied the bin). The vacuum
    /// returns to a resting `Idle` state, ready for new commands.
    ///
    /// # Errors
    /// [`CommandError::IllegalTransition`] if there is no fault to clear.
    pub const fn clear_error(&mut self) -> Result<VacuumState, CommandError> {
        if self.error.is_none() {
            return Err(CommandError::IllegalTransition);
        }
        self.error = None;
        self.state = VacuumState::Idle;
        Ok(self.state)
    }

    /// Feed a fresh battery reading and let the machine react. If the vacuum is
    /// actively cleaning and the battery has fallen to the auto-return
    /// threshold, it abandons the clean and heads home on its own — returning
    /// `true` to tell the caller an auto-return was triggered.
    pub const fn update_battery(&mut self, battery: Battery) -> bool {
        self.battery = battery;
        if self.error.is_none() && self.state.is_cleaning() && battery.is_low() {
            self.state = VacuumState::Returning;
            return true;
        }
        false
    }

    /// Hand control to a person for manual driving (remote control). Allowed
    /// from any non-faulted state.
    ///
    /// # Errors
    /// [`CommandError::Faulted`] if a fault is active.
    pub const fn enter_manual(&mut self) -> Result<VacuumState, CommandError> {
        if self.error.is_some() {
            return Err(CommandError::Faulted);
        }
        self.state = VacuumState::Manual;
        Ok(self.state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battery::ChargeDirection;
    use crate::map::{Segment, VacuumMap};

    fn full_battery() -> Battery {
        Battery::new(90, ChargeDirection::Discharging).expect("valid")
    }

    fn home_map() -> VacuumMap {
        VacuumMap::new(vec![
            Segment::new(1, "Kitchen"),
            Segment::new(2, "Living room"),
        ])
    }

    fn full_vac(state: VacuumState) -> Vacuum {
        Vacuum::with_state(VacuumFeatures::full(), state, full_battery())
    }

    // ---- start / pause / stop / return -----------------------------------

    #[test]
    fn start_from_docked_goes_cleaning() {
        let mut v = full_vac(VacuumState::Docked);
        assert_eq!(v.apply(VacuumCommand::Start), Ok(VacuumState::Cleaning));
    }

    #[test]
    fn start_from_idle_and_paused_goes_cleaning() {
        let mut idle = full_vac(VacuumState::Idle);
        assert_eq!(idle.apply(VacuumCommand::Start), Ok(VacuumState::Cleaning));
        let mut paused = full_vac(VacuumState::Paused);
        assert_eq!(paused.apply(VacuumCommand::Start), Ok(VacuumState::Cleaning));
    }

    #[test]
    fn start_while_already_cleaning_is_rejected() {
        let mut v = full_vac(VacuumState::Cleaning);
        assert_eq!(
            v.apply(VacuumCommand::Start),
            Err(CommandError::IllegalTransition)
        );
        assert_eq!(v.state(), VacuumState::Cleaning, "rejected start changes nothing");
    }

    #[test]
    fn return_to_base_from_cleaning_goes_returning() {
        let mut v = full_vac(VacuumState::Cleaning);
        assert_eq!(
            v.apply(VacuumCommand::ReturnToBase),
            Ok(VacuumState::Returning)
        );
    }

    #[test]
    fn reaching_dock_settles_to_docked() {
        let mut v = full_vac(VacuumState::Returning);
        v.reached_dock();
        assert_eq!(v.state(), VacuumState::Docked);
        assert!(v.state().is_docked());
    }

    #[test]
    fn pause_then_resume_clean() {
        let mut v = full_vac(VacuumState::Cleaning);
        assert_eq!(v.apply(VacuumCommand::Pause), Ok(VacuumState::Paused));
        assert_eq!(v.apply(VacuumCommand::Start), Ok(VacuumState::Cleaning));
    }

    #[test]
    fn pause_while_docked_is_rejected() {
        let mut v = full_vac(VacuumState::Docked);
        assert_eq!(
            v.apply(VacuumCommand::Pause),
            Err(CommandError::IllegalTransition)
        );
    }

    #[test]
    fn stop_parks_the_vacuum_idle() {
        let mut v = full_vac(VacuumState::Cleaning);
        assert_eq!(v.apply(VacuumCommand::Stop), Ok(VacuumState::Idle));
    }

    #[test]
    fn stop_while_docked_is_rejected() {
        let mut v = full_vac(VacuumState::Docked);
        assert_eq!(
            v.apply(VacuumCommand::Stop),
            Err(CommandError::IllegalTransition)
        );
    }

    #[test]
    fn return_when_already_docked_is_a_noop_success() {
        let mut v = full_vac(VacuumState::Docked);
        assert_eq!(
            v.apply(VacuumCommand::ReturnToBase),
            Ok(VacuumState::Docked)
        );
    }

    // ---- spot cleaning + capability ---------------------------------------

    #[test]
    fn spot_clean_from_idle() {
        let mut v = full_vac(VacuumState::Idle);
        assert_eq!(
            v.apply(VacuumCommand::CleanSpot),
            Ok(VacuumState::SpotCleaning)
        );
    }

    #[test]
    fn spot_clean_unsupported_is_rejected() {
        let feats = VacuumFeatures { supports_spot: false, ..VacuumFeatures::basic() };
        let mut v = Vacuum::with_state(feats, VacuumState::Idle, full_battery());
        assert_eq!(
            v.apply(VacuumCommand::CleanSpot),
            Err(CommandError::Unsupported)
        );
    }

    // ---- fan-speed gating -------------------------------------------------

    #[test]
    fn set_supported_fan_speed_takes_effect() {
        let mut v = full_vac(VacuumState::Idle);
        assert_eq!(
            v.apply(VacuumCommand::SetFanSpeed(FanSpeed::Turbo)),
            Ok(VacuumState::Idle)
        );
        assert_eq!(v.fan_speed(), FanSpeed::Turbo);
    }

    #[test]
    fn set_unsupported_fan_speed_is_rejected() {
        let mut v = Vacuum::with_state(
            VacuumFeatures::basic(),
            VacuumState::Idle,
            full_battery(),
        );
        assert_eq!(
            v.apply(VacuumCommand::SetFanSpeed(FanSpeed::Turbo)),
            Err(CommandError::Unsupported)
        );
        assert_eq!(v.fan_speed(), FanSpeed::Medium, "rejected change leaves default");
    }

    // ---- segment / zone cleaning + validation -----------------------------

    #[test]
    fn clean_known_rooms_goes_cleaning() {
        let mut v = Vacuum::docked(VacuumFeatures::full(), full_battery(), home_map());
        assert_eq!(v.clean_rooms(&[1, 2]), Ok(VacuumState::Cleaning));
    }

    #[test]
    fn clean_unknown_room_is_rejected_by_validation() {
        let mut v = Vacuum::docked(VacuumFeatures::full(), full_battery(), home_map());
        assert_eq!(
            v.clean_rooms(&[1, 9]),
            Err(CommandError::BadCleanRequest(
                SegmentRequestError::UnknownSegments(vec![9])
            ))
        );
        assert_eq!(v.state(), VacuumState::Docked, "rejected clean changes nothing");
    }

    #[test]
    fn clean_rooms_unsupported_on_basic_unit() {
        let mut v = Vacuum::with_state(
            VacuumFeatures::basic(),
            VacuumState::Idle,
            full_battery(),
        );
        assert_eq!(v.clean_rooms(&[1]), Err(CommandError::Unsupported));
    }

    #[test]
    fn empty_segment_request_is_rejected() {
        let mut v = Vacuum::docked(VacuumFeatures::full(), full_battery(), home_map());
        assert_eq!(
            v.clean_rooms(&[]),
            Err(CommandError::BadCleanRequest(SegmentRequestError::Empty))
        );
    }

    #[test]
    fn clean_zones_goes_cleaning() {
        let mut v = Vacuum::docked(VacuumFeatures::full(), full_battery(), home_map());
        let z = Zone::new(0, 0, 100, 100).expect("valid");
        assert_eq!(v.clean_zones(&[z]), Ok(VacuumState::Cleaning));
    }

    #[test]
    fn empty_zone_request_is_rejected() {
        let mut v = Vacuum::docked(VacuumFeatures::full(), full_battery(), home_map());
        assert_eq!(
            v.clean_zones(&[]),
            Err(CommandError::BadCleanRequest(SegmentRequestError::Empty))
        );
    }

    // ---- battery ----------------------------------------------------------

    #[test]
    fn low_battery_while_cleaning_auto_returns() {
        let mut v = full_vac(VacuumState::Cleaning);
        let low = Battery::new(15, ChargeDirection::Discharging).expect("valid");
        let triggered = v.update_battery(low);
        assert!(triggered, "low battery while cleaning must trigger auto-return");
        assert_eq!(v.state(), VacuumState::Returning);
    }

    #[test]
    fn low_battery_while_idle_does_not_return() {
        let mut v = full_vac(VacuumState::Idle);
        let low = Battery::new(15, ChargeDirection::Discharging).expect("valid");
        assert!(!v.update_battery(low));
        assert_eq!(v.state(), VacuumState::Idle);
    }

    #[test]
    fn start_blocked_when_battery_too_low() {
        let low = Battery::new(10, ChargeDirection::Discharging).expect("valid");
        let mut v = Vacuum::with_state(VacuumFeatures::full(), VacuumState::Docked, low);
        assert_eq!(v.apply(VacuumCommand::Start), Err(CommandError::BatteryTooLow));
    }

    #[test]
    fn charging_battery_never_forces_return() {
        let mut v = full_vac(VacuumState::Cleaning);
        let charging_low = Battery::new(10, ChargeDirection::Charging).expect("valid");
        assert!(!v.update_battery(charging_low));
        assert_eq!(v.state(), VacuumState::Cleaning);
    }

    // ---- error gating + clearing ------------------------------------------

    #[test]
    fn fault_enters_error_state() {
        let mut v = full_vac(VacuumState::Cleaning);
        v.report_error(ErrorCode::BrushStuck);
        assert_eq!(v.state(), VacuumState::Error);
        assert_eq!(v.error(), Some(ErrorCode::BrushStuck));
        assert!(v.state().needs_attention());
    }

    #[test]
    fn cleaning_commands_gated_while_faulted() {
        let mut v = full_vac(VacuumState::Cleaning);
        v.report_error(ErrorCode::WheelStuck);
        assert_eq!(v.apply(VacuumCommand::Start), Err(CommandError::Faulted));
        assert_eq!(v.apply(VacuumCommand::CleanSpot), Err(CommandError::Faulted));
        assert_eq!(v.clean_rooms(&[1]), Err(CommandError::Faulted));
    }

    #[test]
    fn locate_and_return_allowed_while_faulted() {
        let mut v = full_vac(VacuumState::Cleaning);
        v.report_error(ErrorCode::Lost);
        // Locate is allowed and does not change state.
        assert_eq!(v.apply(VacuumCommand::Locate), Ok(VacuumState::Error));
        // Return-to-base is allowed (try to send it home).
        assert_eq!(
            v.apply(VacuumCommand::ReturnToBase),
            Ok(VacuumState::Returning)
        );
    }

    #[test]
    fn clear_error_returns_to_idle() {
        let mut v = full_vac(VacuumState::Cleaning);
        v.report_error(ErrorCode::BinFull);
        assert_eq!(v.clear_error(), Ok(VacuumState::Idle));
        assert_eq!(v.error(), None);
        // And now commands flow again.
        assert_eq!(v.apply(VacuumCommand::Start), Ok(VacuumState::Cleaning));
    }

    #[test]
    fn clearing_with_no_fault_is_rejected() {
        let mut v = full_vac(VacuumState::Idle);
        assert_eq!(v.clear_error(), Err(CommandError::IllegalTransition));
    }

    #[test]
    fn reached_dock_ignored_while_faulted() {
        let mut v = full_vac(VacuumState::Returning);
        v.report_error(ErrorCode::Trapped);
        v.reached_dock();
        assert_eq!(v.state(), VacuumState::Error, "a fault is not silently docked away");
    }

    // ---- manual -----------------------------------------------------------

    #[test]
    fn enter_manual_from_idle() {
        let mut v = full_vac(VacuumState::Idle);
        assert_eq!(v.enter_manual(), Ok(VacuumState::Manual));
        // And manual can be stopped back to idle.
        assert_eq!(v.apply(VacuumCommand::Stop), Ok(VacuumState::Idle));
    }

    #[test]
    fn enter_manual_blocked_while_faulted() {
        let mut v = full_vac(VacuumState::Idle);
        v.report_error(ErrorCode::Generic);
        assert_eq!(v.enter_manual(), Err(CommandError::Faulted));
    }
}
