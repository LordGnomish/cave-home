//! The front-door call state machine: apply a [`DoorbellEvent`], advance the
//! ring timeout, and reject illegal transitions.
//!
//! Modelled on how a doorbell/intercom interaction actually flows: a press (or
//! motion) starts the bell ringing; a household member answers, declines, or
//! lets it ring out; an answered call ends. The machine is the brain that the
//! camera, audio and notification adapters compose with — it refuses
//! transitions that make no sense (you cannot answer a bell that is not
//! ringing, end a call that never started, etc.) rather than guessing.
//!
//! # Time model
//!
//! The machine reads no clock. The caller advances time by calling
//! [`CallMachine::tick`] with the absolute whole-second [`Tick`] "now". When a
//! ring has been unanswered for at least the configured timeout, the visit
//! resolves to [`CallState::Missed`]. This keeps the logic pure: the timeout
//! boundary is checked against an explicit tick, not a real timer. (A caller
//! that prefers to push the timeout itself may instead apply
//! [`DoorbellEvent::VisitorTimeout`].)

use crate::event::{CallState, DoorbellEvent, Tick};

/// Why a doorbell event could not be applied from the current state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallError {
    /// The event is meaningless from the current state — e.g. answering when
    /// the bell is not ringing, or ending a call that is not in progress.
    IllegalTransition,
}

impl core::fmt::Display for CallError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::IllegalTransition => f.write_str("the front door cannot do that right now"),
        }
    }
}

impl std::error::Error for CallError {}

/// A single front-door interaction's state plus the bookkeeping the ring
/// timeout needs.
#[derive(Debug, Clone)]
pub struct CallMachine {
    state: CallState,
    /// How many whole seconds an unanswered ring may last before the visit is
    /// recorded as missed.
    ring_timeout: Tick,
    /// The tick at which the current ring began. `None` whenever the door is
    /// not ringing.
    ringing_since: Option<Tick>,
}

impl CallMachine {
    /// Build an idle machine whose unanswered rings time out after
    /// `ring_timeout` whole seconds.
    ///
    /// A `ring_timeout` of zero means a ring times out the instant the caller
    /// next advances time to (or past) the ring's start tick.
    #[must_use]
    pub const fn new(ring_timeout: Tick) -> Self {
        Self { state: CallState::Idle, ring_timeout, ringing_since: None }
    }

    #[must_use]
    pub const fn state(&self) -> CallState {
        self.state
    }

    #[must_use]
    pub const fn ring_timeout(&self) -> Tick {
        self.ring_timeout
    }

    /// The tick the current ring started at, if the door is ringing.
    #[must_use]
    pub const fn ringing_since(&self) -> Option<Tick> {
        self.ringing_since
    }

    /// Apply a door / household event at absolute time `now`.
    ///
    /// `now` only matters for the events that *start* a ring ([`DoorbellEvent::ButtonPressed`]
    /// and [`DoorbellEvent::MotionDetected`]); it stamps the ring so a later
    /// [`CallMachine::tick`] can decide whether the timeout has elapsed.
    ///
    /// # Errors
    /// [`CallError::IllegalTransition`] if the event makes no sense from the
    /// current state.
    pub fn apply(&mut self, event: DoorbellEvent, now: Tick) -> Result<CallState, CallError> {
        let next = match (self.state, event) {
            // A press or motion at rest (or while already ringing) (re)starts
            // the ring. While already ringing it simply refreshes the timer —
            // a real "should this even ring again?" decision is the de-dup
            // layer's job (see crate::cooldown), not the state machine's.
            (CallState::Idle | CallState::Ringing, DoorbellEvent::ButtonPressed)
            | (CallState::Idle | CallState::Ringing, DoorbellEvent::MotionDetected) => {
                self.ringing_since = Some(now);
                CallState::Ringing
            }
            // The household picks up a ringing bell.
            (CallState::Ringing, DoorbellEvent::CallAnswered) => {
                self.ringing_since = None;
                CallState::Answered
            }
            // The household turns a ringing visitor away.
            (CallState::Ringing, DoorbellEvent::CallDeclined) => {
                self.ringing_since = None;
                CallState::Declined
            }
            // The ring went unanswered past the timeout.
            (CallState::Ringing, DoorbellEvent::VisitorTimeout) => {
                self.ringing_since = None;
                CallState::Missed
            }
            // An answered call is hung up.
            (CallState::Answered, DoorbellEvent::CallEnded) => CallState::Ended,
            // Everything else is illegal: answering/declining/ending when not
            // in the right state, timing out a non-ring, etc.
            _ => return Err(CallError::IllegalTransition),
        };
        self.state = next;
        Ok(next)
    }

    /// Advance time to absolute tick `now`, resolving an unanswered ring to
    /// [`CallState::Missed`] once it has rung for at least the configured
    /// timeout. Returns the (possibly unchanged) current state.
    ///
    /// Idempotent and monotonic: ticking with a `now` earlier than the ring's
    /// start, or ticking a settled/idle machine, changes nothing.
    pub fn tick(&mut self, now: Tick) -> CallState {
        if self.state == CallState::Ringing {
            if let Some(started) = self.ringing_since {
                let rung_for = now.saturating_sub(started);
                if rung_for >= self.ring_timeout {
                    self.ringing_since = None;
                    self.state = CallState::Missed;
                }
            }
        }
        self.state
    }

    /// Reset the machine to [`CallState::Idle`], ready for the next visitor.
    /// The household calls this after acknowledging a terminal outcome.
    pub fn reset(&mut self) {
        self.state = CallState::Idle;
        self.ringing_since = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_ring_answer_end_cycle() {
        let mut m = CallMachine::new(30);
        assert_eq!(m.state(), CallState::Idle);

        assert_eq!(m.apply(DoorbellEvent::ButtonPressed, 0).unwrap(), CallState::Ringing);
        assert_eq!(m.ringing_since(), Some(0));

        assert_eq!(m.apply(DoorbellEvent::CallAnswered, 5).unwrap(), CallState::Answered);
        assert_eq!(m.ringing_since(), None);

        assert_eq!(m.apply(DoorbellEvent::CallEnded, 90).unwrap(), CallState::Ended);
        assert!(m.state().is_terminal());
    }

    #[test]
    fn ring_then_decline() {
        let mut m = CallMachine::new(30);
        m.apply(DoorbellEvent::ButtonPressed, 0).unwrap();
        assert_eq!(m.apply(DoorbellEvent::CallDeclined, 3).unwrap(), CallState::Declined);
    }

    #[test]
    fn motion_can_start_a_ring() {
        let mut m = CallMachine::new(30);
        assert_eq!(m.apply(DoorbellEvent::MotionDetected, 10).unwrap(), CallState::Ringing);
        assert_eq!(m.ringing_since(), Some(10));
    }

    #[test]
    fn unanswered_ring_times_out_to_missed_at_boundary() {
        let mut m = CallMachine::new(30);
        m.apply(DoorbellEvent::ButtonPressed, 100).unwrap();
        // One second short of the timeout: still ringing.
        assert_eq!(m.tick(129), CallState::Ringing);
        // Exactly at the timeout boundary: missed.
        assert_eq!(m.tick(130), CallState::Missed);
        assert_eq!(m.ringing_since(), None);
    }

    #[test]
    fn timeout_event_path_also_misses() {
        let mut m = CallMachine::new(30);
        m.apply(DoorbellEvent::ButtonPressed, 0).unwrap();
        assert_eq!(m.apply(DoorbellEvent::VisitorTimeout, 31).unwrap(), CallState::Missed);
    }

    #[test]
    fn zero_timeout_misses_on_first_tick() {
        let mut m = CallMachine::new(0);
        m.apply(DoorbellEvent::ButtonPressed, 7).unwrap();
        assert_eq!(m.tick(7), CallState::Missed);
    }

    #[test]
    fn tick_before_ring_start_does_not_underflow_or_miss() {
        let mut m = CallMachine::new(30);
        m.apply(DoorbellEvent::ButtonPressed, 100).unwrap();
        // A clock that hands back an earlier "now" must not panic or miss.
        assert_eq!(m.tick(50), CallState::Ringing);
    }

    #[test]
    fn answering_after_timeout_window_is_still_allowed_until_ticked() {
        // The miss only happens when the caller advances time. Until then a
        // late pickup is honoured.
        let mut m = CallMachine::new(30);
        m.apply(DoorbellEvent::ButtonPressed, 0).unwrap();
        assert_eq!(m.apply(DoorbellEvent::CallAnswered, 999).unwrap(), CallState::Answered);
    }

    #[test]
    fn tick_on_answered_call_never_misses() {
        let mut m = CallMachine::new(30);
        m.apply(DoorbellEvent::ButtonPressed, 0).unwrap();
        m.apply(DoorbellEvent::CallAnswered, 1).unwrap();
        assert_eq!(m.tick(10_000), CallState::Answered);
    }

    #[test]
    fn re_press_while_ringing_refreshes_the_timer() {
        let mut m = CallMachine::new(30);
        m.apply(DoorbellEvent::ButtonPressed, 0).unwrap();
        // A second press at t=20 restarts the 30 s window.
        m.apply(DoorbellEvent::ButtonPressed, 20).unwrap();
        assert_eq!(m.ringing_since(), Some(20));
        // t=49 is only 29 s after the refresh: still ringing.
        assert_eq!(m.tick(49), CallState::Ringing);
        assert_eq!(m.tick(50), CallState::Missed);
    }

    #[test]
    fn answering_when_idle_is_illegal() {
        let mut m = CallMachine::new(30);
        assert_eq!(m.apply(DoorbellEvent::CallAnswered, 0), Err(CallError::IllegalTransition));
    }

    #[test]
    fn ending_a_call_that_never_started_is_illegal() {
        let mut m = CallMachine::new(30);
        m.apply(DoorbellEvent::ButtonPressed, 0).unwrap();
        // Still only ringing — there is no live call to end.
        assert_eq!(m.apply(DoorbellEvent::CallEnded, 1), Err(CallError::IllegalTransition));
    }

    #[test]
    fn declining_an_answered_call_is_illegal() {
        let mut m = CallMachine::new(30);
        m.apply(DoorbellEvent::ButtonPressed, 0).unwrap();
        m.apply(DoorbellEvent::CallAnswered, 1).unwrap();
        assert_eq!(m.apply(DoorbellEvent::CallDeclined, 2), Err(CallError::IllegalTransition));
    }

    #[test]
    fn timing_out_a_non_ring_is_illegal() {
        let mut m = CallMachine::new(30);
        assert_eq!(m.apply(DoorbellEvent::VisitorTimeout, 0), Err(CallError::IllegalTransition));
    }

    #[test]
    fn events_on_a_terminal_state_are_illegal_until_reset() {
        let mut m = CallMachine::new(30);
        m.apply(DoorbellEvent::ButtonPressed, 0).unwrap();
        m.apply(DoorbellEvent::CallDeclined, 1).unwrap();
        assert_eq!(m.apply(DoorbellEvent::CallAnswered, 2), Err(CallError::IllegalTransition));
        m.reset();
        assert_eq!(m.state(), CallState::Idle);
        // After reset the door rings again cleanly.
        assert_eq!(m.apply(DoorbellEvent::ButtonPressed, 3).unwrap(), CallState::Ringing);
    }
}
