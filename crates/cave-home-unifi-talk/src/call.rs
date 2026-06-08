//! The call lifecycle state machine.
//!
//! A single call walks through a small set of [`CallState`]s driven by
//! [`CallEvent`]s. The machine is the brain the SIP/RTP transport (deferred,
//! see the parity manifest) composes with: it refuses transitions that make no
//! sense — you cannot answer a call that is not ringing, hold a call that is
//! not connected, resume one that is not on hold — rather than guessing.
//!
//! # Time model
//!
//! The machine reads no clock. The caller advances time by calling
//! [`CallMachine::tick`] with the absolute whole-second [`Tick`] "now". When a
//! ring has been unanswered for at least the configured timeout, the call
//! resolves to [`CallState::Missed`] or [`CallState::Voicemail`] according to
//! the [`Disposition`] the call was built with. This keeps the logic pure:
//! the timeout boundary is checked against an explicit tick, not a real timer.

/// Absolute time in whole seconds, on the caller's monotonic clock.
pub type Tick = u64;

/// What an unanswered ring resolves to once it times out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// Record the call as a missed call (no voicemail box, or it is disabled).
    Missed,
    /// Roll the unanswered call to voicemail.
    Voicemail,
}

/// The lifecycle state of a single call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallState {
    /// No call in progress.
    Idle,
    /// The phone / intercom is ringing, waiting for someone to pick up.
    Ringing,
    /// Someone answered; the two ends are being joined (media not yet flowing).
    Connecting,
    /// The call is live — people are talking.
    Active,
    /// The call is connected but parked on hold.
    Held,
    /// The call finished normally.
    Ended,
    /// The call rang out unanswered with no voicemail.
    Missed,
    /// The call rang out unanswered and rolled to voicemail.
    Voicemail,
}

impl CallState {
    /// Whether this is a terminal outcome — the call is over and no further
    /// events apply until the machine is [`CallMachine::reset`].
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Ended | Self::Missed | Self::Voicemail)
    }

    /// Whether the call is connected in some form (being joined, talking, or on
    /// hold) — i.e. media exists or is about to.
    #[must_use]
    pub const fn is_connected(self) -> bool {
        matches!(self, Self::Connecting | Self::Active | Self::Held)
    }
}

/// Kind of transfer being performed on a connected call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferKind {
    /// Blind transfer: hand the call straight to the target and drop out.
    Blind,
    /// Attended transfer: the current party speaks to the target first, then
    /// completes the hand-off.
    Attended,
}

/// An event applied to a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallEvent {
    /// An inbound call begins ringing.
    Incoming,
    /// An outbound call begins ringing the far end.
    Outgoing,
    /// The ringing call is answered.
    Answer,
    /// The far end (or the connecting step) completes the media join.
    Connected,
    /// A ringing call is rejected by the household.
    Reject,
    /// A connected call is hung up by either party.
    Hangup,
    /// A connected, talking call is parked on hold.
    Hold,
    /// A held call is taken off hold.
    Resume,
    /// A connected call is handed to another extension.
    Transfer(TransferKind),
    /// The ring-no-answer timeout fired (the caller may apply this explicitly
    /// instead of advancing [`CallMachine::tick`]).
    Timeout,
}

/// Why a [`CallEvent`] could not be applied from the current state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallError {
    /// The event is meaningless from the current state — e.g. answering a call
    /// that is not ringing, or holding a call that is not connected.
    IllegalTransition,
}

impl core::fmt::Display for CallError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::IllegalTransition => f.write_str("the phone cannot do that right now"),
        }
    }
}

impl std::error::Error for CallError {}

/// A single call's state plus the bookkeeping the ring timeout needs.
#[derive(Debug, Clone)]
pub struct CallMachine {
    state: CallState,
    /// How many whole seconds an unanswered ring may last before it resolves.
    ring_timeout: Tick,
    /// What an unanswered ring resolves to.
    disposition: Disposition,
    /// The tick at which the current ring began. `None` whenever not ringing.
    ringing_since: Option<Tick>,
}

impl CallMachine {
    /// Build an idle machine whose unanswered rings time out after
    /// `ring_timeout` whole seconds and then resolve per `disposition`.
    ///
    /// A `ring_timeout` of zero means a ring times out the instant the caller
    /// next advances time to (or past) the ring's start tick.
    #[must_use]
    pub const fn new(ring_timeout: Tick, disposition: Disposition) -> Self {
        Self { state: CallState::Idle, ring_timeout, disposition, ringing_since: None }
    }

    #[must_use]
    pub const fn state(&self) -> CallState {
        self.state
    }

    #[must_use]
    pub const fn disposition(&self) -> Disposition {
        self.disposition
    }

    #[must_use]
    pub const fn ring_timeout(&self) -> Tick {
        self.ring_timeout
    }

    /// The tick the current ring started at, if the call is ringing.
    #[must_use]
    pub const fn ringing_since(&self) -> Option<Tick> {
        self.ringing_since
    }

    /// The outcome an unanswered ring resolves to.
    #[must_use]
    const fn unanswered_outcome(&self) -> CallState {
        match self.disposition {
            Disposition::Missed => CallState::Missed,
            Disposition::Voicemail => CallState::Voicemail,
        }
    }

    /// Apply a call event at absolute time `now`.
    ///
    /// `now` only matters for the events that *start* a ring
    /// ([`CallEvent::Incoming`] / [`CallEvent::Outgoing`]); it stamps the ring
    /// so a later [`CallMachine::tick`] can decide whether the timeout has
    /// elapsed.
    ///
    /// # Errors
    /// [`CallError::IllegalTransition`] if the event makes no sense from the
    /// current state.
    pub fn apply(&mut self, event: CallEvent, now: Tick) -> Result<CallState, CallError> {
        let next = match (self.state, event) {
            // A call starts ringing from rest.
            (CallState::Idle, CallEvent::Incoming | CallEvent::Outgoing) => {
                self.ringing_since = Some(now);
                CallState::Ringing
            }
            // Someone picks up — we move to connecting (media not yet joined).
            (CallState::Ringing, CallEvent::Answer) => {
                self.ringing_since = None;
                CallState::Connecting
            }
            // The media join completes.
            (CallState::Connecting, CallEvent::Connected) => CallState::Active,
            // A ringing call is rejected by the household.
            (CallState::Ringing, CallEvent::Reject) => {
                self.ringing_since = None;
                CallState::Ended
            }
            // The ring went unanswered past the timeout.
            (CallState::Ringing, CallEvent::Timeout) => {
                self.ringing_since = None;
                self.unanswered_outcome()
            }
            // A talking call is parked on hold.
            (CallState::Active, CallEvent::Hold) => CallState::Held,
            // A held call is resumed.
            (CallState::Held, CallEvent::Resume) => CallState::Active,
            // A connected call (talking or held) is transferred away. The
            // transfer hands the call off; from this machine's point of view the
            // local leg ends.
            (CallState::Active | CallState::Held, CallEvent::Transfer(_)) => CallState::Ended,
            // A connected call is hung up.
            (
                CallState::Connecting | CallState::Active | CallState::Held,
                CallEvent::Hangup,
            ) => CallState::Ended,
            // Everything else is illegal.
            _ => return Err(CallError::IllegalTransition),
        };
        self.state = next;
        Ok(next)
    }

    /// Advance time to absolute tick `now`, resolving an unanswered ring once
    /// it has rung for at least the configured timeout. Returns the (possibly
    /// unchanged) current state.
    ///
    /// Idempotent and monotonic: ticking with a `now` earlier than the ring's
    /// start, or ticking a settled / idle / connected machine, changes nothing.
    pub fn tick(&mut self, now: Tick) -> CallState {
        if self.state == CallState::Ringing {
            if let Some(started) = self.ringing_since {
                let rung_for = now.saturating_sub(started);
                if rung_for >= self.ring_timeout {
                    self.ringing_since = None;
                    self.state = self.unanswered_outcome();
                }
            }
        }
        self.state
    }

    /// Reset the machine to [`CallState::Idle`], ready for the next call.
    pub fn reset(&mut self) {
        self.state = CallState::Idle;
        self.ringing_since = None;
    }
}

/// A three-way / conference membership model.
///
/// Pure bookkeeping over which extensions are joined to a single conference,
/// capped at a maximum size (UniFi Talk's three-way is the common case, but the
/// cap is configurable). Joining an already-present member, or joining past the
/// cap, is rejected so the transport never has to reconcile a bad roster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conference {
    members: Vec<String>,
    max_size: usize,
}

/// Why a conference membership change was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConferenceError {
    /// The conference is already at its size cap.
    Full,
    /// That party is already in the conference.
    AlreadyJoined,
    /// That party is not in the conference (cannot remove).
    NotJoined,
}

impl core::fmt::Display for ConferenceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Full => f.write_str("the call is already full"),
            Self::AlreadyJoined => f.write_str("that person is already on the call"),
            Self::NotJoined => f.write_str("that person is not on the call"),
        }
    }
}

impl std::error::Error for ConferenceError {}

impl Conference {
    /// An empty conference holding at most `max_size` parties. A `max_size` of
    /// zero or one is clamped up to one (a conference of nobody is meaningless).
    #[must_use]
    pub fn new(max_size: usize) -> Self {
        Self { members: Vec::new(), max_size: max_size.max(1) }
    }

    /// A standard three-way conference.
    #[must_use]
    pub fn three_way() -> Self {
        Self::new(3)
    }

    #[must_use]
    pub fn members(&self) -> &[String] {
        &self.members
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.members.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    #[must_use]
    pub fn contains(&self, party: &str) -> bool {
        self.members.iter().any(|m| m == party)
    }

    /// Add a party to the conference.
    ///
    /// # Errors
    /// [`ConferenceError::AlreadyJoined`] if already present;
    /// [`ConferenceError::Full`] if at the size cap.
    pub fn join(&mut self, party: impl Into<String>) -> Result<(), ConferenceError> {
        let party = party.into();
        if self.contains(&party) {
            return Err(ConferenceError::AlreadyJoined);
        }
        if self.members.len() >= self.max_size {
            return Err(ConferenceError::Full);
        }
        self.members.push(party);
        Ok(())
    }

    /// Remove a party from the conference.
    ///
    /// # Errors
    /// [`ConferenceError::NotJoined`] if the party is not present.
    pub fn leave(&mut self, party: &str) -> Result<(), ConferenceError> {
        match self.members.iter().position(|m| m == party) {
            Some(i) => {
                self.members.remove(i);
                Ok(())
            }
            None => Err(ConferenceError::NotJoined),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- state machine: the happy path ------------------------------------

    #[test]
    fn full_incoming_answer_connect_end_cycle() {
        let mut m = CallMachine::new(30, Disposition::Voicemail);
        assert_eq!(m.state(), CallState::Idle);
        assert_eq!(m.apply(CallEvent::Incoming, 0).unwrap(), CallState::Ringing);
        assert_eq!(m.ringing_since(), Some(0));
        assert_eq!(m.apply(CallEvent::Answer, 4).unwrap(), CallState::Connecting);
        assert_eq!(m.ringing_since(), None);
        assert_eq!(m.apply(CallEvent::Connected, 5).unwrap(), CallState::Active);
        assert_eq!(m.apply(CallEvent::Hangup, 90).unwrap(), CallState::Ended);
        assert!(m.state().is_terminal());
    }

    #[test]
    fn outgoing_call_rings_then_connects() {
        let mut m = CallMachine::new(30, Disposition::Missed);
        assert_eq!(m.apply(CallEvent::Outgoing, 0).unwrap(), CallState::Ringing);
        assert_eq!(m.apply(CallEvent::Answer, 2).unwrap(), CallState::Connecting);
        assert_eq!(m.apply(CallEvent::Connected, 3).unwrap(), CallState::Active);
    }

    #[test]
    fn reject_ends_a_ringing_call() {
        let mut m = CallMachine::new(30, Disposition::Voicemail);
        m.apply(CallEvent::Incoming, 0).unwrap();
        assert_eq!(m.apply(CallEvent::Reject, 1).unwrap(), CallState::Ended);
    }

    // ---- hold / resume -----------------------------------------------------

    #[test]
    fn hold_then_resume_round_trips() {
        let mut m = CallMachine::new(30, Disposition::Voicemail);
        m.apply(CallEvent::Incoming, 0).unwrap();
        m.apply(CallEvent::Answer, 1).unwrap();
        m.apply(CallEvent::Connected, 2).unwrap();
        assert_eq!(m.apply(CallEvent::Hold, 10).unwrap(), CallState::Held);
        assert_eq!(m.apply(CallEvent::Resume, 20).unwrap(), CallState::Active);
    }

    #[test]
    fn holding_a_call_that_is_not_active_is_illegal() {
        let mut m = CallMachine::new(30, Disposition::Voicemail);
        m.apply(CallEvent::Incoming, 0).unwrap();
        // Still only ringing — nothing to hold.
        assert_eq!(m.apply(CallEvent::Hold, 1), Err(CallError::IllegalTransition));
    }

    #[test]
    fn resuming_a_call_that_is_not_held_is_illegal() {
        let mut m = CallMachine::new(30, Disposition::Voicemail);
        m.apply(CallEvent::Incoming, 0).unwrap();
        m.apply(CallEvent::Answer, 1).unwrap();
        m.apply(CallEvent::Connected, 2).unwrap();
        assert_eq!(m.apply(CallEvent::Resume, 3), Err(CallError::IllegalTransition));
    }

    // ---- transfer ----------------------------------------------------------

    #[test]
    fn blind_transfer_from_active_ends_local_leg() {
        let mut m = CallMachine::new(30, Disposition::Voicemail);
        m.apply(CallEvent::Incoming, 0).unwrap();
        m.apply(CallEvent::Answer, 1).unwrap();
        m.apply(CallEvent::Connected, 2).unwrap();
        assert_eq!(
            m.apply(CallEvent::Transfer(TransferKind::Blind), 5).unwrap(),
            CallState::Ended
        );
    }

    #[test]
    fn attended_transfer_from_hold_is_allowed() {
        let mut m = CallMachine::new(30, Disposition::Voicemail);
        m.apply(CallEvent::Incoming, 0).unwrap();
        m.apply(CallEvent::Answer, 1).unwrap();
        m.apply(CallEvent::Connected, 2).unwrap();
        m.apply(CallEvent::Hold, 3).unwrap();
        assert_eq!(
            m.apply(CallEvent::Transfer(TransferKind::Attended), 6).unwrap(),
            CallState::Ended
        );
    }

    #[test]
    fn transfer_before_connect_is_illegal() {
        let mut m = CallMachine::new(30, Disposition::Voicemail);
        m.apply(CallEvent::Incoming, 0).unwrap();
        assert_eq!(
            m.apply(CallEvent::Transfer(TransferKind::Blind), 1),
            Err(CallError::IllegalTransition)
        );
    }

    // ---- ring-no-answer -> missed / voicemail ------------------------------

    #[test]
    fn unanswered_ring_rolls_to_voicemail_at_boundary() {
        let mut m = CallMachine::new(30, Disposition::Voicemail);
        m.apply(CallEvent::Incoming, 100).unwrap();
        assert_eq!(m.tick(129), CallState::Ringing, "one second short");
        assert_eq!(m.tick(130), CallState::Voicemail, "at the boundary");
        assert_eq!(m.ringing_since(), None);
    }

    #[test]
    fn unanswered_ring_rolls_to_missed_when_no_voicemail() {
        let mut m = CallMachine::new(30, Disposition::Missed);
        m.apply(CallEvent::Incoming, 0).unwrap();
        assert_eq!(m.tick(30), CallState::Missed);
    }

    #[test]
    fn timeout_event_path_matches_disposition() {
        let mut m = CallMachine::new(30, Disposition::Voicemail);
        m.apply(CallEvent::Incoming, 0).unwrap();
        assert_eq!(m.apply(CallEvent::Timeout, 31).unwrap(), CallState::Voicemail);
    }

    #[test]
    fn zero_timeout_resolves_on_first_tick() {
        let mut m = CallMachine::new(0, Disposition::Missed);
        m.apply(CallEvent::Incoming, 7).unwrap();
        assert_eq!(m.tick(7), CallState::Missed);
    }

    #[test]
    fn tick_before_ring_start_does_not_underflow() {
        let mut m = CallMachine::new(30, Disposition::Voicemail);
        m.apply(CallEvent::Incoming, 100).unwrap();
        assert_eq!(m.tick(50), CallState::Ringing);
    }

    #[test]
    fn late_answer_is_honoured_until_ticked() {
        let mut m = CallMachine::new(30, Disposition::Voicemail);
        m.apply(CallEvent::Incoming, 0).unwrap();
        // Past the timeout window, but no tick has resolved it yet.
        assert_eq!(m.apply(CallEvent::Answer, 999).unwrap(), CallState::Connecting);
    }

    #[test]
    fn tick_on_active_call_never_resolves() {
        let mut m = CallMachine::new(30, Disposition::Voicemail);
        m.apply(CallEvent::Incoming, 0).unwrap();
        m.apply(CallEvent::Answer, 1).unwrap();
        m.apply(CallEvent::Connected, 2).unwrap();
        assert_eq!(m.tick(100_000), CallState::Active);
    }

    // ---- illegal transitions ----------------------------------------------

    #[test]
    fn answering_when_idle_is_illegal() {
        let mut m = CallMachine::new(30, Disposition::Voicemail);
        assert_eq!(m.apply(CallEvent::Answer, 0), Err(CallError::IllegalTransition));
    }

    #[test]
    fn hanging_up_a_call_that_never_connected_is_illegal() {
        let mut m = CallMachine::new(30, Disposition::Voicemail);
        m.apply(CallEvent::Incoming, 0).unwrap();
        // A ringing call is rejected, not hung up.
        assert_eq!(m.apply(CallEvent::Hangup, 1), Err(CallError::IllegalTransition));
    }

    #[test]
    fn double_incoming_is_illegal() {
        let mut m = CallMachine::new(30, Disposition::Voicemail);
        m.apply(CallEvent::Incoming, 0).unwrap();
        assert_eq!(m.apply(CallEvent::Incoming, 1), Err(CallError::IllegalTransition));
    }

    #[test]
    fn events_on_terminal_state_are_illegal_until_reset() {
        let mut m = CallMachine::new(30, Disposition::Voicemail);
        m.apply(CallEvent::Incoming, 0).unwrap();
        m.apply(CallEvent::Reject, 1).unwrap();
        assert_eq!(m.apply(CallEvent::Answer, 2), Err(CallError::IllegalTransition));
        m.reset();
        assert_eq!(m.state(), CallState::Idle);
        assert_eq!(m.apply(CallEvent::Incoming, 3).unwrap(), CallState::Ringing);
    }

    #[test]
    fn state_classification_helpers() {
        assert!(CallState::Ended.is_terminal());
        assert!(CallState::Missed.is_terminal());
        assert!(CallState::Voicemail.is_terminal());
        assert!(!CallState::Active.is_terminal());
        assert!(CallState::Active.is_connected());
        assert!(CallState::Held.is_connected());
        assert!(!CallState::Ringing.is_connected());
    }

    // ---- conference --------------------------------------------------------

    #[test]
    fn three_way_starts_empty_and_caps_at_three() {
        let mut c = Conference::three_way();
        assert!(c.is_empty());
        c.join("101").unwrap();
        c.join("102").unwrap();
        c.join("103").unwrap();
        assert_eq!(c.len(), 3);
        assert_eq!(c.join("104"), Err(ConferenceError::Full));
    }

    #[test]
    fn cannot_join_the_same_party_twice() {
        let mut c = Conference::three_way();
        c.join("101").unwrap();
        assert_eq!(c.join("101"), Err(ConferenceError::AlreadyJoined));
    }

    #[test]
    fn leaving_makes_room_again() {
        let mut c = Conference::three_way();
        c.join("101").unwrap();
        c.join("102").unwrap();
        c.join("103").unwrap();
        c.leave("102").unwrap();
        assert!(!c.contains("102"));
        // Now there is room for a new party.
        c.join("104").unwrap();
        assert_eq!(c.len(), 3);
    }

    #[test]
    fn leaving_a_non_member_is_rejected() {
        let mut c = Conference::three_way();
        c.join("101").unwrap();
        assert_eq!(c.leave("999"), Err(ConferenceError::NotJoined));
    }

    #[test]
    fn conference_size_is_clamped_to_at_least_one() {
        let mut c = Conference::new(0);
        c.join("101").unwrap();
        assert_eq!(c.join("102"), Err(ConferenceError::Full));
    }
}
