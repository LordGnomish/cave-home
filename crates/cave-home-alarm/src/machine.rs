//! The alarm-panel state machine: apply commands, advance the exit/entry/siren
//! timers, surface sensor trips, and gate everything on the user code.
//!
//! Modelled on the Home Assistant `alarm_control_panel` entity domain
//! (Apache-2.0): arming runs an *exit delay* (`Arming`); a watched sensor trip
//! runs an *entry delay* (`Pending`) before the alarm sounds; `Triggered` is
//! the safety-critical "alarm is going off" surface; and a valid code is always
//! required to disarm. The machine is the safety brain — it refuses illegal
//! transitions and bad codes rather than guessing.
//!
//! # Time model
//!
//! The machine reads no clock. The caller advances time by calling
//! [`AlarmPanel::tick`] with the whole seconds that have elapsed since the last
//! tick. This keeps the safety logic pure and exhaustively testable: every
//! exit/entry/siren boundary is checked against an explicit `tick`, not a real
//! timer. Sensor/siren hardware and a real clock are Phase-1b adapters (see
//! `parity.manifest.toml`).

use crate::code::{CodeCredential, CodeVerdict, UserCode};
use crate::config::{PanelConfig, Seconds};
use crate::state::{AlarmCommand, AlarmState};

/// Why a command was rejected by the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlarmError {
    /// The command is meaningless from the current state (e.g. arming a panel
    /// that is already armed in that mode, or disarming an already-disarmed
    /// panel).
    IllegalTransition,
    /// A command required a code but the presented code was wrong, missing, or
    /// the keypad is locked out after too many wrong attempts.
    CodeRejected,
}

impl core::fmt::Display for AlarmError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::IllegalTransition => f.write_str("the alarm cannot do that right now"),
            Self::CodeRejected => f.write_str("the code was not accepted"),
        }
    }
}

impl std::error::Error for AlarmError {}

/// A sensor trip reported to the panel: which kind of zone fired. The actual
/// door/window/motion hardware is a Phase-1b adapter; the machine only needs to
/// know whether the trip came from a zone the *current* armed mode treats as
/// instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    /// A perimeter zone (door / window). Subject to the entry delay unless the
    /// armed mode's perimeter is configured instant.
    Perimeter,
    /// An interior motion zone. In home/night modes this is often bypassed; an
    /// instant interior zone sounds the alarm immediately.
    Interior,
}

/// The alarm control panel: its current state, configuration, enrolled code,
/// and the bookkeeping needed to advance the exit/entry/siren timers.
#[derive(Debug, Clone)]
pub struct AlarmPanel {
    state: AlarmState,
    config: PanelConfig,
    code: CodeCredential,
    /// Whole seconds spent in the current transient/triggered phase.
    elapsed: Seconds,
    /// The armed state a successful `Arming` countdown is heading toward.
    pending_arm: Option<AlarmState>,
    /// The armed state to return to after a trip resolves (entry delay / siren).
    prior_armed: Option<AlarmState>,
}

impl AlarmPanel {
    /// Build a disarmed panel with the given configuration and enrolled code.
    #[must_use]
    pub fn new(config: PanelConfig, code: CodeCredential) -> Self {
        Self {
            state: AlarmState::Disarmed,
            config,
            code,
            elapsed: 0,
            pending_arm: None,
            prior_armed: None,
        }
    }

    #[must_use]
    pub const fn state(&self) -> AlarmState {
        self.state
    }

    #[must_use]
    pub const fn config(&self) -> &PanelConfig {
        &self.config
    }

    /// Seconds elapsed in the current transient or triggered phase. Zero in a
    /// settled state.
    #[must_use]
    pub const fn elapsed(&self) -> Seconds {
        self.elapsed
    }

    /// Whether the keypad is currently locked out after too many wrong codes.
    #[must_use]
    pub const fn is_locked_out(&self) -> bool {
        self.code.is_locked_out()
    }

    /// Apply a household command **without** a code.
    ///
    /// This is only legal for arm commands when the configuration does not
    /// require a code to arm. Disarm always requires a code, and a panic
    /// [`AlarmCommand::Trigger`] is accepted code-free (it only *raises* the
    /// alarm, never lowers it).
    ///
    /// # Errors
    /// [`AlarmError::CodeRejected`] if the command needs a code;
    /// [`AlarmError::IllegalTransition`] if the command makes no sense from the
    /// current state.
    pub fn apply(&mut self, command: AlarmCommand) -> Result<AlarmState, AlarmError> {
        if self.command_requires_code(command) {
            return Err(AlarmError::CodeRejected);
        }
        self.apply_validated(command)
    }

    /// Apply a household command, verifying the presented code first.
    ///
    /// The legality of the command is checked *before* an attempt is spent, so
    /// an illegal command never burns the keypad lock-out budget. The code is
    /// then verified, and the command applied only on
    /// [`CodeVerdict::Accepted`].
    ///
    /// # Errors
    /// [`AlarmError::CodeRejected`] if the code is wrong or locked out;
    /// [`AlarmError::IllegalTransition`] if the command makes no sense from the
    /// current state.
    pub fn apply_with_code(
        &mut self,
        command: AlarmCommand,
        presented: &UserCode,
    ) -> Result<AlarmState, AlarmError> {
        // Check legality before spending an attempt.
        self.check_legal(command)?;
        match self.code.verify(presented) {
            CodeVerdict::Accepted => self.apply_validated(command),
            CodeVerdict::Rejected | CodeVerdict::LockedOut => Err(AlarmError::CodeRejected),
        }
    }

    /// Whether this command would need a code given the current configuration.
    /// Disarm always does; arming does only when configured; panic-trigger
    /// never does.
    const fn command_requires_code(&self, command: AlarmCommand) -> bool {
        match command {
            AlarmCommand::Disarm => true,
            AlarmCommand::Trigger => false,
            // arm commands
            _ => self.config.arm_requires_code(),
        }
    }

    /// Pure legality check: is `command` sensible from the current state,
    /// ignoring the code?
    fn check_legal(&self, command: AlarmCommand) -> Result<(), AlarmError> {
        match command {
            // Disarm is legal from anything except an already-disarmed panel.
            AlarmCommand::Disarm => {
                if matches!(self.state, AlarmState::Disarmed) {
                    Err(AlarmError::IllegalTransition)
                } else {
                    Ok(())
                }
            }
            // Panic-trigger is legal unless already triggered.
            AlarmCommand::Trigger => {
                if matches!(self.state, AlarmState::Triggered) {
                    Err(AlarmError::IllegalTransition)
                } else {
                    Ok(())
                }
            }
            // Arm commands: legal from disarmed, or to switch between arm modes;
            // not legal once already in the requested settled mode, and not
            // while pending/triggered (disarm first).
            _ => {
                let target = command.target_armed_state();
                if matches!(self.state, AlarmState::Pending | AlarmState::Triggered) {
                    return Err(AlarmError::IllegalTransition);
                }
                if self.state.is_armed() && self.state == target.unwrap_or(self.state) {
                    return Err(AlarmError::IllegalTransition);
                }
                Ok(())
            }
        }
    }

    /// Apply a command that has already passed code + legality checks.
    fn apply_validated(&mut self, command: AlarmCommand) -> Result<AlarmState, AlarmError> {
        self.check_legal(command)?;
        match command {
            AlarmCommand::Disarm => {
                self.state = AlarmState::Disarmed;
                self.elapsed = 0;
                self.pending_arm = None;
                self.prior_armed = None;
                // A successful disarm clears any keypad lock-out: the household
                // proved they hold a valid code, so the brute-force guard resets.
                self.code.reset();
            }
            AlarmCommand::Trigger => {
                // Remember where to return to, then sound immediately.
                if self.state.is_armed() {
                    self.prior_armed = Some(self.state);
                }
                self.state = AlarmState::Triggered;
                self.elapsed = 0;
            }
            arm => {
                let target = arm.target_armed_state().unwrap_or(AlarmState::ArmedAway);
                self.pending_arm = Some(target);
                self.elapsed = 0;
                if self.config.exit_delay() == 0 {
                    // No exit delay: arm immediately.
                    self.state = target;
                    self.pending_arm = None;
                } else {
                    self.state = AlarmState::Arming;
                }
            }
        }
        Ok(self.state)
    }

    /// Report that a watched sensor in `zone` tripped.
    ///
    /// Only meaningful in a settled armed state. In an instant zone (per config
    /// for the current mode) the alarm sounds immediately; otherwise the panel
    /// enters `Pending` to run the entry delay. A trip while disarmed, arming,
    /// already pending or already triggered is ignored and the state is
    /// returned unchanged.
    pub fn sensor_trip(&mut self, zone: Zone) -> AlarmState {
        if !self.state.is_armed() {
            return self.state;
        }
        let armed = self.state;
        let instant = self.zone_is_instant(armed, zone);
        // Interior motion is bypassed in home mode (people are moving around).
        if matches!(zone, Zone::Interior) && matches!(armed, AlarmState::ArmedHome) && !instant {
            return self.state;
        }
        self.prior_armed = Some(armed);
        self.elapsed = 0;
        if instant || self.config.entry_delay() == 0 {
            self.state = AlarmState::Triggered;
        } else {
            self.state = AlarmState::Pending;
        }
        self.state
    }

    /// Whether a trip while in `armed` should fire instantly.
    ///
    /// The instant flags belong to the home/night modes (an instant
    /// perimeter/interior zone that sounds without the entry delay). The
    /// `zone` is accepted for symmetry with `sensor_trip` and future
    /// per-zone-class policy, but today both zone classes honour the same
    /// mode-level instant flag.
    fn zone_is_instant(&self, armed: AlarmState, _zone: Zone) -> bool {
        let instant_home = matches!(armed, AlarmState::ArmedHome) && self.config.home_instant();
        let instant_night = matches!(armed, AlarmState::ArmedNight) && self.config.night_instant();
        instant_home || instant_night
    }

    /// Advance the panel by `secs` whole seconds, resolving any transient or
    /// triggered phase whose configured duration has now elapsed.
    ///
    /// - `Arming`  → the requested armed state once the exit delay elapses.
    /// - `Pending` → `Triggered` once the entry delay elapses.
    /// - `Triggered` → the prior armed state once the siren time elapses
    ///   (unless the config says stay triggered, or there is no prior armed
    ///   state to return to, in which case it stays triggered for a human).
    ///
    /// Settled states ignore ticks. Returns the (possibly new) state.
    pub fn tick(&mut self, secs: Seconds) -> AlarmState {
        if !self.state.is_transient() && !self.state.is_triggered() {
            return self.state;
        }
        self.elapsed = self.elapsed.saturating_add(secs);
        match self.state {
            AlarmState::Arming => {
                if self.elapsed >= self.config.exit_delay() {
                    self.state = self.pending_arm.unwrap_or(AlarmState::ArmedAway);
                    self.pending_arm = None;
                    self.elapsed = 0;
                }
            }
            AlarmState::Pending => {
                if self.elapsed >= self.config.entry_delay() {
                    self.state = AlarmState::Triggered;
                    self.elapsed = 0;
                }
            }
            AlarmState::Triggered => {
                if self.elapsed >= self.config.trigger_time() {
                    match (self.config.stay_triggered(), self.prior_armed) {
                        (false, Some(prior)) => {
                            self.state = prior;
                            self.prior_armed = None;
                            self.elapsed = 0;
                        }
                        // Stay triggered (config) or nothing to return to: a
                        // human must disarm. Hold the triggered state.
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(s: &str) -> UserCode {
        UserCode::parse(s).expect("valid test code")
    }

    /// A panel with a known code and the given config.
    fn panel(cfg: PanelConfig) -> AlarmPanel {
        AlarmPanel::new(cfg, CodeCredential::enroll(&code("1379")))
    }

    /// exit 10, entry 5, siren 20, code required, no instant, auto-return.
    fn standard_cfg() -> PanelConfig {
        PanelConfig::new(10, 5, 20, true, false, false, false).expect("valid")
    }

    #[test]
    fn full_cycle_arm_pending_triggered_disarm() {
        let mut p = panel(standard_cfg());
        // Arm away (code required).
        assert_eq!(
            p.apply_with_code(AlarmCommand::ArmAway, &code("1379")),
            Ok(AlarmState::Arming)
        );
        // Exit delay elapses -> ArmedAway.
        assert_eq!(p.tick(10), AlarmState::ArmedAway);
        assert!(p.state().is_armed());
        // A door opens -> Pending (entry delay).
        assert_eq!(p.sensor_trip(Zone::Perimeter), AlarmState::Pending);
        // Entry delay elapses without disarm -> Triggered.
        assert_eq!(p.tick(5), AlarmState::Triggered);
        assert!(p.state().is_triggered());
        // Disarm with the right code clears it.
        assert_eq!(
            p.apply_with_code(AlarmCommand::Disarm, &code("1379")),
            Ok(AlarmState::Disarmed)
        );
    }

    #[test]
    fn exit_delay_boundary_one_second_short_stays_arming() {
        let mut p = panel(standard_cfg());
        p.apply_with_code(AlarmCommand::ArmAway, &code("1379")).expect("arm");
        assert_eq!(p.tick(9), AlarmState::Arming, "1s short of exit delay");
        assert_eq!(p.tick(1), AlarmState::ArmedAway, "exactly at exit delay arms");
    }

    #[test]
    fn entry_delay_boundary_disarm_in_time_prevents_trigger() {
        let mut p = panel(standard_cfg());
        p.apply_with_code(AlarmCommand::ArmAway, &code("1379")).expect("arm");
        p.tick(10);
        p.sensor_trip(Zone::Perimeter);
        assert_eq!(p.tick(4), AlarmState::Pending, "still within entry delay");
        // Disarm just before the entry delay elapses.
        assert_eq!(
            p.apply_with_code(AlarmCommand::Disarm, &code("1379")),
            Ok(AlarmState::Disarmed)
        );
    }

    #[test]
    fn entry_delay_elapses_exactly_triggers() {
        let mut p = panel(standard_cfg());
        p.apply_with_code(AlarmCommand::ArmAway, &code("1379")).expect("arm");
        p.tick(10);
        p.sensor_trip(Zone::Perimeter);
        assert_eq!(p.tick(5), AlarmState::Triggered);
    }

    #[test]
    fn zero_exit_delay_arms_immediately() {
        let cfg = PanelConfig::new(0, 5, 20, true, false, false, false).expect("valid");
        let mut p = panel(cfg);
        assert_eq!(
            p.apply_with_code(AlarmCommand::ArmAway, &code("1379")),
            Ok(AlarmState::ArmedAway),
            "no exit delay arms without an Arming phase"
        );
    }

    #[test]
    fn zero_entry_delay_triggers_immediately_on_trip() {
        let cfg = PanelConfig::new(10, 0, 20, true, false, false, false).expect("valid");
        let mut p = panel(cfg);
        p.apply_with_code(AlarmCommand::ArmAway, &code("1379")).expect("arm");
        p.tick(10);
        assert_eq!(p.sensor_trip(Zone::Perimeter), AlarmState::Triggered);
    }

    #[test]
    fn instant_home_zone_triggers_without_entry_delay() {
        // Home instant: an interior trip in home mode sounds immediately.
        let cfg = PanelConfig::new(0, 30, 20, true, true, false, false).expect("valid");
        let mut p = panel(cfg);
        p.apply_with_code(AlarmCommand::ArmHome, &code("1379")).expect("arm home");
        assert_eq!(p.state(), AlarmState::ArmedHome);
        assert_eq!(p.sensor_trip(Zone::Interior), AlarmState::Triggered);
    }

    #[test]
    fn home_mode_bypasses_interior_motion_when_not_instant() {
        let cfg = PanelConfig::new(0, 30, 20, true, false, false, false).expect("valid");
        let mut p = panel(cfg);
        p.apply_with_code(AlarmCommand::ArmHome, &code("1379")).expect("arm home");
        // Interior motion in home mode (not instant) is bypassed: people move.
        assert_eq!(p.sensor_trip(Zone::Interior), AlarmState::ArmedHome);
        // But the perimeter is still watched.
        assert_eq!(p.sensor_trip(Zone::Perimeter), AlarmState::Pending);
    }

    #[test]
    fn instant_night_zone_triggers_without_entry_delay() {
        let cfg = PanelConfig::new(0, 30, 20, true, false, true, false).expect("valid");
        let mut p = panel(cfg);
        p.apply_with_code(AlarmCommand::ArmNight, &code("1379")).expect("arm night");
        assert_eq!(p.sensor_trip(Zone::Interior), AlarmState::Triggered);
    }

    #[test]
    fn triggered_auto_returns_to_prior_armed_state_after_siren() {
        let mut p = panel(standard_cfg());
        p.apply_with_code(AlarmCommand::ArmAway, &code("1379")).expect("arm");
        p.tick(10);
        p.sensor_trip(Zone::Perimeter);
        p.tick(5); // -> Triggered
        assert_eq!(p.state(), AlarmState::Triggered);
        // Siren time elapses -> back to ArmedAway (auto-return config).
        assert_eq!(p.tick(20), AlarmState::ArmedAway);
    }

    #[test]
    fn triggered_stays_when_config_says_stay() {
        let cfg = PanelConfig::new(10, 5, 20, true, false, false, true).expect("valid");
        let mut p = panel(cfg);
        p.apply_with_code(AlarmCommand::ArmAway, &code("1379")).expect("arm");
        p.tick(10);
        p.sensor_trip(Zone::Perimeter);
        p.tick(5); // -> Triggered
        // Even after the siren time, it stays triggered for a human to disarm.
        assert_eq!(p.tick(100), AlarmState::Triggered);
    }

    #[test]
    fn wrong_code_rejected_and_state_unchanged() {
        let mut p = panel(standard_cfg());
        assert_eq!(
            p.apply_with_code(AlarmCommand::ArmAway, &code("0000")),
            Err(AlarmError::CodeRejected)
        );
        assert_eq!(p.state(), AlarmState::Disarmed, "rejected arm changes nothing");
    }

    #[test]
    fn disarm_requires_correct_code() {
        let mut p = panel(standard_cfg());
        p.apply_with_code(AlarmCommand::ArmAway, &code("1379")).expect("arm");
        p.tick(10);
        assert_eq!(
            p.apply_with_code(AlarmCommand::Disarm, &code("0000")),
            Err(AlarmError::CodeRejected)
        );
        assert_eq!(p.state(), AlarmState::ArmedAway, "wrong disarm leaves armed");
    }

    #[test]
    fn brute_force_lockout_then_correct_code_refused() {
        let cfg = standard_cfg();
        let mut p = AlarmPanel::new(cfg, CodeCredential::enroll_with_limit(&code("1379"), 3));
        p.apply_with_code(AlarmCommand::ArmAway, &code("1379")).expect("arm");
        p.tick(10);
        // Three wrong disarms -> locked out.
        for _ in 0..3 {
            let _ = p.apply_with_code(AlarmCommand::Disarm, &code("0000"));
        }
        assert!(p.is_locked_out());
        // The correct code is now refused while locked out.
        assert_eq!(
            p.apply_with_code(AlarmCommand::Disarm, &code("1379")),
            Err(AlarmError::CodeRejected)
        );
        assert_eq!(p.state(), AlarmState::ArmedAway);
    }

    #[test]
    fn illegal_command_does_not_spend_an_attempt() {
        let mut p = AlarmPanel::new(standard_cfg(), CodeCredential::enroll_with_limit(&code("1379"), 3));
        // Disarming an already-disarmed panel is illegal; it must not burn the
        // keypad budget even with a wrong code.
        assert_eq!(
            p.apply_with_code(AlarmCommand::Disarm, &code("0000")),
            Err(AlarmError::IllegalTransition)
        );
        // Now exhaust the budget on a legal-but-wrong path to prove it was full.
        p.apply_with_code(AlarmCommand::ArmAway, &code("1379")).expect("arm");
        p.tick(10);
        for _ in 0..3 {
            let _ = p.apply_with_code(AlarmCommand::Disarm, &code("0000"));
        }
        assert!(p.is_locked_out(), "exactly 3 attempts after the illegal one locked it out");
    }

    #[test]
    fn code_optional_arm_when_configured() {
        let cfg = PanelConfig::new(10, 5, 20, false, false, false, false).expect("valid");
        let mut p = panel(cfg);
        // arm_requires_code is false: code-free arm is accepted.
        assert_eq!(p.apply(AlarmCommand::ArmAway), Ok(AlarmState::Arming));
    }

    #[test]
    fn code_free_arm_refused_when_code_required() {
        let mut p = panel(standard_cfg()); // arm_requires_code = true
        assert_eq!(p.apply(AlarmCommand::ArmAway), Err(AlarmError::CodeRejected));
        assert_eq!(p.state(), AlarmState::Disarmed);
    }

    #[test]
    fn disarm_without_code_always_refused() {
        let cfg = PanelConfig::new(10, 5, 20, false, false, false, false).expect("valid");
        let mut p = panel(cfg);
        p.apply(AlarmCommand::ArmAway).expect("code-optional arm");
        p.tick(10);
        // Even with arm_requires_code = false, disarm needs a code.
        assert_eq!(p.apply(AlarmCommand::Disarm), Err(AlarmError::CodeRejected));
    }

    #[test]
    fn cannot_arm_into_the_same_mode() {
        let mut p = panel(standard_cfg());
        p.apply_with_code(AlarmCommand::ArmAway, &code("1379")).expect("arm");
        p.tick(10);
        assert_eq!(p.state(), AlarmState::ArmedAway);
        // Re-arming the same mode is a no-op illegal transition.
        assert_eq!(
            p.apply_with_code(AlarmCommand::ArmAway, &code("1379")),
            Err(AlarmError::IllegalTransition)
        );
    }

    #[test]
    fn can_switch_between_armed_modes() {
        let mut p = panel(standard_cfg());
        p.apply_with_code(AlarmCommand::ArmAway, &code("1379")).expect("arm away");
        p.tick(10);
        // Switching away->home re-runs the exit delay.
        assert_eq!(
            p.apply_with_code(AlarmCommand::ArmHome, &code("1379")),
            Ok(AlarmState::Arming)
        );
        assert_eq!(p.tick(10), AlarmState::ArmedHome);
    }

    #[test]
    fn cannot_arm_while_pending() {
        let mut p = panel(standard_cfg());
        p.apply_with_code(AlarmCommand::ArmAway, &code("1379")).expect("arm");
        p.tick(10);
        p.sensor_trip(Zone::Perimeter); // Pending
        assert_eq!(
            p.apply_with_code(AlarmCommand::ArmHome, &code("1379")),
            Err(AlarmError::IllegalTransition)
        );
    }

    #[test]
    fn panic_trigger_is_code_free_and_sounds_immediately() {
        let mut p = panel(standard_cfg());
        assert_eq!(p.apply(AlarmCommand::Trigger), Ok(AlarmState::Triggered));
    }

    #[test]
    fn panic_trigger_while_armed_returns_to_that_mode_after_siren() {
        let mut p = panel(standard_cfg());
        p.apply_with_code(AlarmCommand::ArmNight, &code("1379")).expect("arm night");
        p.tick(10);
        assert_eq!(p.apply(AlarmCommand::Trigger), Ok(AlarmState::Triggered));
        // After the siren time, return to the night mode it was in.
        assert_eq!(p.tick(20), AlarmState::ArmedNight);
    }

    #[test]
    fn panic_trigger_from_disarmed_stays_triggered_with_no_prior_state() {
        let mut p = panel(standard_cfg());
        p.apply(AlarmCommand::Trigger).expect("panic");
        // No prior armed state to return to: it holds for a human to disarm.
        assert_eq!(p.tick(1000), AlarmState::Triggered);
    }

    #[test]
    fn cannot_trigger_when_already_triggered() {
        let mut p = panel(standard_cfg());
        p.apply(AlarmCommand::Trigger).expect("panic");
        assert_eq!(p.apply(AlarmCommand::Trigger), Err(AlarmError::IllegalTransition));
    }

    #[test]
    fn ticks_in_settled_state_are_noops() {
        let mut p = panel(standard_cfg());
        p.apply_with_code(AlarmCommand::ArmAway, &code("1379")).expect("arm");
        p.tick(10); // ArmedAway
        assert_eq!(p.tick(9999), AlarmState::ArmedAway, "settled state ignores time");
        assert_eq!(p.elapsed(), 0);
    }

    #[test]
    fn sensor_trip_while_disarmed_is_ignored() {
        let mut p = panel(standard_cfg());
        assert_eq!(p.sensor_trip(Zone::Perimeter), AlarmState::Disarmed);
        assert_eq!(p.sensor_trip(Zone::Interior), AlarmState::Disarmed);
    }

    #[test]
    fn sensor_trip_during_arming_is_ignored() {
        // While still in the exit delay nobody is "intruding".
        let mut p = panel(standard_cfg());
        p.apply_with_code(AlarmCommand::ArmAway, &code("1379")).expect("arm");
        assert_eq!(p.state(), AlarmState::Arming);
        assert_eq!(p.sensor_trip(Zone::Perimeter), AlarmState::Arming);
    }

    #[test]
    fn disarm_clears_keypad_lockout() {
        let mut p = AlarmPanel::new(standard_cfg(), CodeCredential::enroll_with_limit(&code("1379"), 2));
        p.apply_with_code(AlarmCommand::ArmAway, &code("1379")).expect("arm");
        p.tick(10);
        // One wrong attempt (limit 2, not yet locked out).
        let _ = p.apply_with_code(AlarmCommand::Disarm, &code("0000"));
        assert!(!p.is_locked_out());
        // A correct disarm resets the failure counter.
        assert_eq!(
            p.apply_with_code(AlarmCommand::Disarm, &code("1379")),
            Ok(AlarmState::Disarmed)
        );
        // Re-arm and prove the counter was reset (2 wrong now needed to lock).
        p.apply_with_code(AlarmCommand::ArmAway, &code("1379")).expect("arm");
        p.tick(10);
        let _ = p.apply_with_code(AlarmCommand::Disarm, &code("0000"));
        assert!(!p.is_locked_out(), "counter was reset by the successful disarm");
    }

    #[test]
    fn vacation_mode_arms_and_watches_everything() {
        let mut p = panel(standard_cfg());
        p.apply_with_code(AlarmCommand::ArmVacation, &code("1379")).expect("arm vacation");
        assert_eq!(p.tick(10), AlarmState::ArmedVacation);
        // Interior motion is NOT bypassed in vacation mode.
        assert_eq!(p.sensor_trip(Zone::Interior), AlarmState::Pending);
    }
}
