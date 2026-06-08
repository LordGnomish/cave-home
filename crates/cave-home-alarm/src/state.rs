//! The alarm-panel state model and the commands a household can issue.
//!
//! These mirror the Home Assistant `alarm_control_panel` entity-domain
//! semantics (Apache-2.0): a panel is in one of a small set of states, the
//! transient states (`Arming`, `Pending`) resolve to a settled state as the
//! exit/entry delay elapses, and `Triggered` is the safety-critical "the alarm
//! is going off" surface.
//!
//! Nothing here touches a vendor, a radio, a sensor or a siren — the wire
//! adapters that drive these transitions (door/window/motion sensors, siren
//! actuators) are Phase-1b (see `parity.manifest.toml`).

/// The state of the alarm control panel.
///
/// Mirrors the HA `alarm_control_panel` state set. Equality and pattern
/// matching are the only comparisons that make sense; there is no natural
/// "more armed than" ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlarmState {
    /// The alarm is off — the house is not being watched.
    Disarmed,
    /// Armed while people are home: perimeter watched, interior motion usually
    /// bypassed.
    ArmedHome,
    /// Armed while the house is empty: everything watched.
    ArmedAway,
    /// Armed for sleeping: perimeter watched, some interior zones bypassed.
    ArmedNight,
    /// Armed for an extended absence: the strictest watch.
    ArmedVacation,
    /// Armed with a household-chosen set of bypassed zones.
    ArmedCustomBypass,
    /// An arm command is in flight: the exit delay is counting down so the
    /// household can leave before the watch begins.
    Arming,
    /// A watched sensor tripped while armed: the entry delay is counting down
    /// so a returning household can disarm before the alarm sounds.
    Pending,
    /// The alarm is sounding. Safety-critical: a sensor tripped and was not
    /// disarmed in time (or an instant zone fired).
    Triggered,
}

impl AlarmState {
    /// Whether this is one of the settled "armed" states (not disarmed, not a
    /// transient countdown, not triggered).
    #[must_use]
    pub const fn is_armed(self) -> bool {
        matches!(
            self,
            Self::ArmedHome
                | Self::ArmedAway
                | Self::ArmedNight
                | Self::ArmedVacation
                | Self::ArmedCustomBypass
        )
    }

    /// Whether this is a transient countdown state expected to resolve once the
    /// configured delay elapses.
    #[must_use]
    pub const fn is_transient(self) -> bool {
        matches!(self, Self::Arming | Self::Pending)
    }

    /// Whether the alarm is actively sounding. The whole point of the panel is
    /// to surface this honestly.
    #[must_use]
    pub const fn is_triggered(self) -> bool {
        matches!(self, Self::Triggered)
    }
}

/// A command a household (or an automation) can issue to the alarm panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlarmCommand {
    /// Turn the alarm off. Always requires a valid code.
    Disarm,
    /// Arm for being home.
    ArmHome,
    /// Arm for an empty house.
    ArmAway,
    /// Arm for sleeping.
    ArmNight,
    /// Arm for an extended absence.
    ArmVacation,
    /// Force the alarm to sound immediately (e.g. a panic button).
    Trigger,
}

impl AlarmCommand {
    /// The settled armed state this arm command targets once its exit delay
    /// elapses. `Disarm` and `Trigger` are not arm commands and return `None`.
    #[must_use]
    pub const fn target_armed_state(self) -> Option<AlarmState> {
        match self {
            Self::ArmHome => Some(AlarmState::ArmedHome),
            Self::ArmAway => Some(AlarmState::ArmedAway),
            Self::ArmNight => Some(AlarmState::ArmedNight),
            Self::ArmVacation => Some(AlarmState::ArmedVacation),
            Self::Disarm | Self::Trigger => None,
        }
    }

    /// Whether this command arms the panel (as opposed to disarming or
    /// panic-triggering it).
    #[must_use]
    pub const fn is_arm(self) -> bool {
        self.target_armed_state().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_STATES: [AlarmState; 9] = [
        AlarmState::Disarmed,
        AlarmState::ArmedHome,
        AlarmState::ArmedAway,
        AlarmState::ArmedNight,
        AlarmState::ArmedVacation,
        AlarmState::ArmedCustomBypass,
        AlarmState::Arming,
        AlarmState::Pending,
        AlarmState::Triggered,
    ];

    #[test]
    fn armed_predicate_matches_only_settled_armed_states() {
        assert!(AlarmState::ArmedHome.is_armed());
        assert!(AlarmState::ArmedAway.is_armed());
        assert!(AlarmState::ArmedNight.is_armed());
        assert!(AlarmState::ArmedVacation.is_armed());
        assert!(AlarmState::ArmedCustomBypass.is_armed());
        assert!(!AlarmState::Disarmed.is_armed());
        assert!(!AlarmState::Arming.is_armed());
        assert!(!AlarmState::Pending.is_armed());
        assert!(!AlarmState::Triggered.is_armed());
    }

    #[test]
    fn only_arming_and_pending_are_transient() {
        assert!(AlarmState::Arming.is_transient());
        assert!(AlarmState::Pending.is_transient());
        for s in ALL_STATES {
            if !matches!(s, AlarmState::Arming | AlarmState::Pending) {
                assert!(!s.is_transient(), "{s:?} must not be transient");
            }
        }
    }

    #[test]
    fn only_triggered_is_triggered() {
        assert!(AlarmState::Triggered.is_triggered());
        for s in ALL_STATES {
            if !matches!(s, AlarmState::Triggered) {
                assert!(!s.is_triggered(), "{s:?} must not report triggered");
            }
        }
    }

    #[test]
    fn arm_commands_map_to_their_armed_state() {
        assert_eq!(
            AlarmCommand::ArmHome.target_armed_state(),
            Some(AlarmState::ArmedHome)
        );
        assert_eq!(
            AlarmCommand::ArmAway.target_armed_state(),
            Some(AlarmState::ArmedAway)
        );
        assert_eq!(
            AlarmCommand::ArmNight.target_armed_state(),
            Some(AlarmState::ArmedNight)
        );
        assert_eq!(
            AlarmCommand::ArmVacation.target_armed_state(),
            Some(AlarmState::ArmedVacation)
        );
        assert_eq!(AlarmCommand::Disarm.target_armed_state(), None);
        assert_eq!(AlarmCommand::Trigger.target_armed_state(), None);
    }

    #[test]
    fn is_arm_splits_arm_from_non_arm_commands() {
        assert!(AlarmCommand::ArmHome.is_arm());
        assert!(AlarmCommand::ArmVacation.is_arm());
        assert!(!AlarmCommand::Disarm.is_arm());
        assert!(!AlarmCommand::Trigger.is_arm());
    }
}
