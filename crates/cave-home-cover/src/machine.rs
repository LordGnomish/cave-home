//! The cover position state machine.
//!
//! A [`Cover`] holds the device class, its [`Features`], the current openness
//! [`Position`], an optional tilt [`Position`] and a [`CoverState`]. Commands
//! ([`CoverCommand`]) are applied through [`Cover::apply`], which:
//!
//! - rejects commands the device's features don't support,
//! - infers travel direction (`Opening` if the target is more open than now,
//!   `Closing` if less, settling immediately if already there),
//! - lets a `Stop` halt motion at the current position **at any time**,
//! - settles a reached target to the right at-rest state (`Open` / `Closed` /
//!   `Stopped`),
//! - keeps tilt as an independent axis that never moves the main position.
//!
//! Safety: [`Cover::report_obstruction`] forces the cover to `Stopped` at its
//! current position regardless of what it was doing — an obstruction or jam
//! always wins. A `Stop` command is likewise always honoured while moving, even
//! on a device whose `stop` feature flag is false (you can always cut the
//! motor; the flag only governs whether *you* can issue a stand-alone Stop).

use crate::device_class::{DeviceClass, Features};
use crate::position::Position;
use crate::state::CoverState;

/// A command the household (or an automation) can issue to a cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverCommand {
    /// Drive fully open.
    Open,
    /// Drive fully closed.
    Close,
    /// Halt at the current position.
    Stop,
    /// Drive to an exact openness percentage.
    SetPosition(Position),
    /// Tilt the slats to an exact percentage (independent of openness).
    SetTiltPosition(Position),
}

/// Why a command was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandError {
    /// The cover cannot be driven to an exact position (open/close only).
    PositionUnsupported,
    /// The cover has no tiltable slats.
    TiltUnsupported,
}

impl core::fmt::Display for CommandError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::PositionUnsupported => {
                f.write_str("this cover cannot be set to an exact position")
            }
            Self::TiltUnsupported => f.write_str("this cover has no tilt"),
        }
    }
}

impl std::error::Error for CommandError {}

/// A single cover and its live state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cover {
    class: DeviceClass,
    features: Features,
    position: Position,
    tilt: Option<Position>,
    state: CoverState,
}

impl Cover {
    /// Build a cover of `class` with `features`, starting fully closed and at
    /// rest. Tilt starts fully closed (slats shut) if the device tilts, else
    /// there is no tilt axis at all.
    #[must_use]
    pub const fn new(class: DeviceClass, features: Features) -> Self {
        let tilt = if features.tilt { Some(Position::CLOSED) } else { None };
        Self {
            class,
            features,
            position: Position::CLOSED,
            tilt,
            state: CoverState::Closed,
        }
    }

    /// Build a cover of `class` using that class's default features.
    #[must_use]
    pub const fn with_class_defaults(class: DeviceClass) -> Self {
        Self::new(class, class.default_features())
    }

    /// The device class.
    #[must_use]
    pub const fn class(self) -> DeviceClass {
        self.class
    }

    /// The cover's feature set.
    #[must_use]
    pub const fn features(self) -> Features {
        self.features
    }

    /// Current openness.
    #[must_use]
    pub const fn position(self) -> Position {
        self.position
    }

    /// Current tilt, if the device tilts.
    #[must_use]
    pub const fn tilt(self) -> Option<Position> {
        self.tilt
    }

    /// Current motion state.
    #[must_use]
    pub const fn state(self) -> CoverState {
        self.state
    }

    /// Apply a command, mutating the cover in place.
    ///
    /// In this pure-logic model a positional move *completes* synchronously:
    /// after `Open`, the cover is at 100% and `Open`. The transient
    /// `Opening`/`Closing` states are produced by [`Cover::begin`] for callers
    /// (vendor adapters, deferred to Phase 1b) that drive a real motor and
    /// settle later.
    ///
    /// # Errors
    /// Returns [`CommandError`] for a `SetPosition` on a non-positionable cover
    /// or a `SetTiltPosition` on a cover without tilt.
    pub fn apply(&mut self, command: CoverCommand) -> Result<(), CommandError> {
        match command {
            CoverCommand::Open => {
                self.position = Position::OPEN;
                self.state = CoverState::Open;
                Ok(())
            }
            CoverCommand::Close => {
                self.position = Position::CLOSED;
                self.state = CoverState::Closed;
                Ok(())
            }
            // A Stop is always honoured — cutting the motor is universal. It
            // settles to the at-rest state for wherever the cover currently is.
            CoverCommand::Stop => {
                self.state = CoverState::at_rest_for(self.position);
                Ok(())
            }
            CoverCommand::SetPosition(target) => {
                if !self.features.set_position {
                    return Err(CommandError::PositionUnsupported);
                }
                self.position = target;
                self.state = CoverState::at_rest_for(target);
                Ok(())
            }
            CoverCommand::SetTiltPosition(target) => {
                if !self.features.tilt {
                    return Err(CommandError::TiltUnsupported);
                }
                // Tilt is independent: openness and motion are untouched.
                self.tilt = Some(target);
                Ok(())
            }
        }
    }

    /// Begin a positional move *without* completing it, returning the transient
    /// motion state the motor would be in. Used to model the in-flight phase a
    /// real (Phase 1b) motor adapter would report.
    ///
    /// `Open`/`Close` map to their endpoints; `SetPosition` compares the target
    /// against the current openness to infer direction. A target equal to the
    /// current position settles immediately (no motion). `Stop` and tilt are
    /// not motion of the main axis and are delegated to [`Cover::apply`].
    ///
    /// # Errors
    /// Returns [`CommandError`] under the same rules as [`Cover::apply`].
    pub fn begin(&mut self, command: CoverCommand) -> Result<CoverState, CommandError> {
        let target = match command {
            CoverCommand::Open => Position::OPEN,
            CoverCommand::Close => Position::CLOSED,
            CoverCommand::SetPosition(p) => {
                if !self.features.set_position {
                    return Err(CommandError::PositionUnsupported);
                }
                p
            }
            // These are not main-axis motion; defer to apply for the side
            // effect and report the resulting at-rest state.
            CoverCommand::Stop | CoverCommand::SetTiltPosition(_) => {
                self.apply(command)?;
                return Ok(self.state);
            }
        };
        let direction = Self::direction(self.position, target);
        self.state = direction;
        Ok(direction)
    }

    /// Settle an in-flight move at `reached`, picking the right at-rest state.
    /// A motor adapter calls this when the cover stops moving.
    pub fn settle_at(&mut self, reached: Position) {
        self.position = reached;
        self.state = CoverState::at_rest_for(reached);
    }

    /// Force the cover to a halt at its current position because an obstruction
    /// or jam was detected. Safety override: always accepted, whatever the
    /// cover was doing and whatever features it declares.
    pub fn report_obstruction(&mut self) {
        self.state = CoverState::Stopped;
    }

    /// The motion direction implied by travelling from `from` to `to`.
    ///
    /// Equal positions settle to the at-rest state (no spurious motion).
    #[must_use]
    fn direction(from: Position, to: Position) -> CoverState {
        match to.percent().cmp(&from.percent()) {
            core::cmp::Ordering::Greater => CoverState::Opening,
            core::cmp::Ordering::Less => CoverState::Closing,
            core::cmp::Ordering::Equal => CoverState::at_rest_for(to),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(p: u8) -> Position {
        Position::new(p).expect("valid test position")
    }

    #[test]
    fn open_drives_to_full_and_open_state() {
        let mut c = Cover::with_class_defaults(DeviceClass::Shutter);
        c.apply(CoverCommand::Open).unwrap();
        assert_eq!(c.position(), Position::OPEN);
        assert_eq!(c.state(), CoverState::Open);
    }

    #[test]
    fn close_drives_to_zero_and_closed_state() {
        let mut c = Cover::with_class_defaults(DeviceClass::Shutter);
        c.apply(CoverCommand::Open).unwrap();
        c.apply(CoverCommand::Close).unwrap();
        assert_eq!(c.position(), Position::CLOSED);
        assert_eq!(c.state(), CoverState::Closed);
    }

    #[test]
    fn set_position_to_middle_settles_stopped() {
        let mut c = Cover::with_class_defaults(DeviceClass::Shade);
        c.apply(CoverCommand::SetPosition(pos(40))).unwrap();
        assert_eq!(c.position(), pos(40));
        assert_eq!(c.state(), CoverState::Stopped);
    }

    #[test]
    fn set_position_to_full_or_zero_settles_open_or_closed() {
        let mut c = Cover::with_class_defaults(DeviceClass::Shade);
        c.apply(CoverCommand::SetPosition(pos(100))).unwrap();
        assert_eq!(c.state(), CoverState::Open);
        c.apply(CoverCommand::SetPosition(pos(0))).unwrap();
        assert_eq!(c.state(), CoverState::Closed);
    }

    #[test]
    fn direction_inferred_opening_when_target_higher() {
        let mut c = Cover::with_class_defaults(DeviceClass::Shade);
        c.apply(CoverCommand::SetPosition(pos(30))).unwrap();
        let dir = c.begin(CoverCommand::SetPosition(pos(70))).unwrap();
        assert_eq!(dir, CoverState::Opening);
        assert_eq!(c.state(), CoverState::Opening);
    }

    #[test]
    fn direction_inferred_closing_when_target_lower() {
        let mut c = Cover::with_class_defaults(DeviceClass::Shade);
        c.apply(CoverCommand::SetPosition(pos(70))).unwrap();
        let dir = c.begin(CoverCommand::SetPosition(pos(20))).unwrap();
        assert_eq!(dir, CoverState::Closing);
    }

    #[test]
    fn begin_to_same_position_does_not_move() {
        let mut c = Cover::with_class_defaults(DeviceClass::Shade);
        c.apply(CoverCommand::SetPosition(pos(55))).unwrap();
        let dir = c.begin(CoverCommand::SetPosition(pos(55))).unwrap();
        assert_eq!(dir, CoverState::Stopped);
        assert!(!c.state().is_moving());
    }

    #[test]
    fn begin_then_settle_reaches_target() {
        let mut c = Cover::with_class_defaults(DeviceClass::Shade);
        c.begin(CoverCommand::Open).unwrap();
        assert_eq!(c.state(), CoverState::Opening);
        c.settle_at(pos(100));
        assert_eq!(c.state(), CoverState::Open);
        assert_eq!(c.position(), Position::OPEN);
    }

    #[test]
    fn stop_halts_at_current_position_while_moving() {
        let mut c = Cover::with_class_defaults(DeviceClass::Shade);
        c.begin(CoverCommand::Open).unwrap();
        // Motor has travelled to 60% when the household hits stop.
        c.settle_at(pos(60));
        c.begin(CoverCommand::Open).unwrap();
        c.apply(CoverCommand::Stop).unwrap();
        assert_eq!(c.position(), pos(60));
        assert_eq!(c.state(), CoverState::Stopped);
    }

    #[test]
    fn stop_is_accepted_even_without_stop_feature() {
        // A minimal cover has stop=false, yet cutting the motor must work.
        let mut c = Cover::new(DeviceClass::Garage, Features::minimal());
        c.begin(CoverCommand::Open).unwrap();
        assert!(c.apply(CoverCommand::Stop).is_ok());
    }

    #[test]
    fn set_position_rejected_when_unsupported() {
        let mut c = Cover::new(DeviceClass::Garage, Features::minimal());
        assert_eq!(
            c.apply(CoverCommand::SetPosition(pos(50))),
            Err(CommandError::PositionUnsupported)
        );
        // The reject must not have moved anything.
        assert_eq!(c.position(), Position::CLOSED);
        assert_eq!(c.state(), CoverState::Closed);
    }

    #[test]
    fn begin_set_position_rejected_when_unsupported() {
        let mut c = Cover::new(DeviceClass::Garage, Features::minimal());
        assert_eq!(
            c.begin(CoverCommand::SetPosition(pos(50))),
            Err(CommandError::PositionUnsupported)
        );
    }

    #[test]
    fn tilt_rejected_when_unsupported() {
        let mut c = Cover::with_class_defaults(DeviceClass::Shade);
        assert_eq!(
            c.apply(CoverCommand::SetTiltPosition(pos(50))),
            Err(CommandError::TiltUnsupported)
        );
        assert_eq!(c.tilt(), None);
    }

    #[test]
    fn tilt_is_independent_of_openness() {
        let mut c = Cover::with_class_defaults(DeviceClass::Blind);
        c.apply(CoverCommand::SetPosition(pos(80))).unwrap();
        c.apply(CoverCommand::SetTiltPosition(pos(25))).unwrap();
        // Tilting did not disturb the openness or its at-rest state.
        assert_eq!(c.position(), pos(80));
        assert_eq!(c.tilt(), Some(pos(25)));
        assert_eq!(c.state(), CoverState::Stopped);
        // And re-tilting does not move the main axis either.
        c.apply(CoverCommand::SetTiltPosition(pos(100))).unwrap();
        assert_eq!(c.position(), pos(80));
        assert_eq!(c.tilt(), Some(pos(100)));
    }

    #[test]
    fn obstruction_forces_stop_while_opening() {
        let mut c = Cover::with_class_defaults(DeviceClass::Garage);
        c.begin(CoverCommand::Open).unwrap();
        assert_eq!(c.state(), CoverState::Opening);
        c.report_obstruction();
        assert_eq!(c.state(), CoverState::Stopped);
    }

    #[test]
    fn obstruction_forces_stop_while_closing() {
        let mut c = Cover::with_class_defaults(DeviceClass::Garage);
        c.apply(CoverCommand::Open).unwrap();
        c.begin(CoverCommand::Close).unwrap();
        assert_eq!(c.state(), CoverState::Closing);
        c.report_obstruction();
        assert_eq!(c.state(), CoverState::Stopped);
    }

    #[test]
    fn new_starts_closed_and_tilt_present_only_when_supported() {
        let blind = Cover::with_class_defaults(DeviceClass::Blind);
        assert_eq!(blind.state(), CoverState::Closed);
        assert_eq!(blind.tilt(), Some(Position::CLOSED));

        let shade = Cover::with_class_defaults(DeviceClass::Shade);
        assert_eq!(shade.tilt(), None);
    }
}
