//! The doorbell / intercom event vocabulary and the call states a single
//! front-door interaction moves through.
//!
//! Nothing here touches a vendor, a radio, a camera or an audio channel. The
//! wire adapters that *produce* these events (a Reolink / DoorBird / Ring-RTSP
//! button-press webhook, a PIR motion edge) and the two-way SIP/WebRTC audio
//! that an answered call would carry are Phase-1b and network/hardware-bound
//! (see `parity.manifest.toml`, ADR-018). This module is just the typed
//! vocabulary the pure call engine reasons over.

/// A whole-second tick supplied by the caller. The crate reads no clock; every
/// time-dependent decision (ring timeout, motion/ring de-dup cooldown) is taken
/// against an explicit tick so the logic stays pure and exhaustively testable.
pub type Tick = u64;

/// The state of a single front-door interaction.
///
/// A normal answered visit walks `Idle → Ringing → Answered → Ended`. A visit
/// nobody picks up walks `Idle → Ringing → Missed`. A rejected visit walks
/// `Idle → Ringing → Declined`. These are the only states a household ever
/// needs to reason about — the camera, audio and notification machinery hangs
/// off them but never adds new states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallState {
    /// Nothing is happening at the door.
    Idle,
    /// The bell is ringing (or motion raised a call) and is waiting to be
    /// answered. Subject to the ring timeout: ring past it and the visit is
    /// recorded as missed.
    Ringing,
    /// Someone in the household picked up — the two-way call is live.
    Answered,
    /// The bell rang but nobody picked up before the ring timeout elapsed.
    Missed,
    /// The household explicitly turned the visitor away (declined the call).
    Declined,
    /// An answered call finished normally.
    Ended,
}

impl CallState {
    /// Whether the interaction is still live — the bell is ringing or a call is
    /// in progress. Settled outcomes (`Missed` / `Declined` / `Ended`) and the
    /// resting `Idle` state are not active.
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Ringing | Self::Answered)
    }

    /// Whether this is a settled terminal outcome of a visit. A terminal state
    /// is what the visitor log records.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Missed | Self::Declined | Self::Ended)
    }
}

/// Something that happened at the front door (or that the household did about
/// it).
///
/// `ButtonPressed` and `MotionDetected` are *inbound* events from the door;
/// `CallAnswered` / `CallDeclined` / `CallEnded` are *household* actions; and
/// `VisitorTimeout` is the engine's own "nobody came" signal, raised by the
/// caller when the ring has been unanswered past the configured timeout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DoorbellEvent {
    /// The doorbell button was pressed.
    ButtonPressed,
    /// Motion was detected in front of the door (no button press).
    MotionDetected,
    /// Someone in the household answered the call.
    CallAnswered,
    /// Someone in the household turned the visitor away.
    CallDeclined,
    /// An answered call was hung up.
    CallEnded,
    /// The ring went unanswered past the configured timeout.
    VisitorTimeout,
}

impl DoorbellEvent {
    /// Whether this event is one a household member performs (as opposed to an
    /// event the door or the timeout logic raises).
    #[must_use]
    pub const fn is_household_action(self) -> bool {
        matches!(self, Self::CallAnswered | Self::CallDeclined | Self::CallEnded)
    }

    /// Whether this event originates at the door itself (a press or motion).
    #[must_use]
    pub const fn is_door_signal(self) -> bool {
        matches!(self, Self::ButtonPressed | Self::MotionDetected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_STATES: [CallState; 6] = [
        CallState::Idle,
        CallState::Ringing,
        CallState::Answered,
        CallState::Missed,
        CallState::Declined,
        CallState::Ended,
    ];

    #[test]
    fn only_ringing_and_answered_are_active() {
        assert!(CallState::Ringing.is_active());
        assert!(CallState::Answered.is_active());
        for s in ALL_STATES {
            if !matches!(s, CallState::Ringing | CallState::Answered) {
                assert!(!s.is_active(), "{s:?} must not be active");
            }
        }
    }

    #[test]
    fn terminal_states_are_the_settled_outcomes() {
        assert!(CallState::Missed.is_terminal());
        assert!(CallState::Declined.is_terminal());
        assert!(CallState::Ended.is_terminal());
        assert!(!CallState::Idle.is_terminal());
        assert!(!CallState::Ringing.is_terminal());
        assert!(!CallState::Answered.is_terminal());
    }

    #[test]
    fn active_and_terminal_are_disjoint() {
        for s in ALL_STATES {
            assert!(
                !(s.is_active() && s.is_terminal()),
                "{s:?} cannot be both active and terminal"
            );
        }
    }

    #[test]
    fn household_actions_are_classified() {
        assert!(DoorbellEvent::CallAnswered.is_household_action());
        assert!(DoorbellEvent::CallDeclined.is_household_action());
        assert!(DoorbellEvent::CallEnded.is_household_action());
        assert!(!DoorbellEvent::ButtonPressed.is_household_action());
        assert!(!DoorbellEvent::VisitorTimeout.is_household_action());
    }

    #[test]
    fn door_signals_are_classified() {
        assert!(DoorbellEvent::ButtonPressed.is_door_signal());
        assert!(DoorbellEvent::MotionDetected.is_door_signal());
        assert!(!DoorbellEvent::CallAnswered.is_door_signal());
        assert!(!DoorbellEvent::VisitorTimeout.is_door_signal());
    }
}
