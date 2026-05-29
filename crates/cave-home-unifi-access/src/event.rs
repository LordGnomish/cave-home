// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Access-event / log model (ADR-009).
//!
//! Every access attempt — granted or denied — is recorded as an [`AccessEvent`]
//! (who, which door, the outcome reason, a monotonic tick). An [`AccessLog`]
//! keeps a bounded history and offers an *anti-passback hint*: if the same
//! person is granted entry at a door twice in a row without an intervening exit,
//! that is suspicious (a card may have been passed back through a window).

use crate::door::DoorId;
use crate::policy::DenyReason;

/// What happened on an access attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessOutcome {
    /// The person was let through.
    Granted,
    /// The person was refused, for the given reason.
    Denied(DenyReason),
}

impl AccessOutcome {
    /// Whether this outcome let the person through.
    #[must_use]
    pub fn is_granted(&self) -> bool {
        matches!(self, Self::Granted)
    }
}

/// The direction a granted passage went, where the reader can tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Entering the secured area.
    Entry,
    /// Leaving the secured area.
    Exit,
    /// The reader cannot distinguish direction.
    Unknown,
}

/// A single recorded access attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessEvent {
    /// Who attempted access (a person/user identifier).
    pub who: String,
    /// Which door.
    pub door: DoorId,
    /// The outcome.
    pub outcome: AccessOutcome,
    /// Direction of travel, where known.
    pub direction: Direction,
    /// Monotonic tick supplied by the caller (e.g. seconds since boot).
    pub tick: u64,
}

impl AccessEvent {
    /// Record a granted passage.
    #[must_use]
    pub fn granted(who: impl Into<String>, door: DoorId, direction: Direction, tick: u64) -> Self {
        Self {
            who: who.into(),
            door,
            outcome: AccessOutcome::Granted,
            direction,
            tick,
        }
    }

    /// Record a denied attempt.
    #[must_use]
    pub fn denied(who: impl Into<String>, door: DoorId, reason: DenyReason, tick: u64) -> Self {
        Self {
            who: who.into(),
            door,
            outcome: AccessOutcome::Denied(reason),
            direction: Direction::Unknown,
            tick,
        }
    }
}

/// A bounded, chronological access history.
#[derive(Clone, Debug, Default)]
pub struct AccessLog {
    events: Vec<AccessEvent>,
    capacity: usize,
}

impl AccessLog {
    /// Default ring capacity when none is given.
    pub const DEFAULT_CAPACITY: usize = 256;

    /// A new log with the default capacity.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    /// A new log that keeps at most `capacity` most-recent events. A capacity
    /// of zero is treated as one (always keep the latest).
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            events: Vec::new(),
            capacity: capacity.max(1),
        }
    }

    /// Append an event, evicting the oldest if at capacity.
    pub fn record(&mut self, event: AccessEvent) {
        if self.events.len() >= self.capacity {
            self.events.remove(0);
        }
        self.events.push(event);
    }

    /// Number of events currently held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the log is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// All retained events, oldest first.
    #[must_use]
    pub fn events(&self) -> &[AccessEvent] {
        &self.events
    }

    /// The most recent event, if any.
    #[must_use]
    pub fn last(&self) -> Option<&AccessEvent> {
        self.events.last()
    }

    /// Every granted passage by this person, in order.
    #[must_use]
    pub fn granted_for(&self, who: &str) -> Vec<&AccessEvent> {
        self.events
            .iter()
            .filter(|e| e.who == who && e.outcome.is_granted())
            .collect()
    }

    /// Anti-passback hint: would granting `who` an **entry** at `door` right now
    /// be suspicious because their last known *granted* passage was already an
    /// entry (with no exit in between)?
    ///
    /// This is a *hint*, not an enforcement: the caller decides whether to deny,
    /// alert, or ignore. It models the classic "card passed back over the fence"
    /// case where a person appears to enter twice without leaving.
    #[must_use]
    pub fn is_passback_suspicious(&self, who: &str, intended: Direction) -> bool {
        if intended != Direction::Entry {
            return false;
        }
        // Find this person's most recent granted passage with a known direction.
        let last_known = self
            .events
            .iter()
            .rev()
            .find(|e| {
                e.who == who
                    && e.outcome.is_granted()
                    && e.direction != Direction::Unknown
            });
        matches!(last_known, Some(e) if e.direction == Direction::Entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_grant_and_deny() {
        let mut log = AccessLog::new();
        log.record(AccessEvent::granted("alice", DoorId::new("d1"), Direction::Entry, 1));
        log.record(AccessEvent::denied(
            "bob",
            DoorId::new("d1"),
            DenyReason::NoPermission,
            2,
        ));
        assert_eq!(log.len(), 2);
        assert!(!log.is_empty());
        assert_eq!(log.events()[0].who, "alice");
        assert!(log.events()[0].outcome.is_granted());
        assert!(!log.events()[1].outcome.is_granted());
    }

    #[test]
    fn capacity_evicts_oldest() {
        let mut log = AccessLog::with_capacity(2);
        log.record(AccessEvent::granted("a", DoorId::new("d"), Direction::Entry, 1));
        log.record(AccessEvent::granted("b", DoorId::new("d"), Direction::Entry, 2));
        log.record(AccessEvent::granted("c", DoorId::new("d"), Direction::Entry, 3));
        assert_eq!(log.len(), 2);
        assert_eq!(log.events()[0].who, "b"); // "a" evicted
        assert_eq!(log.last().expect("non-empty").who, "c");
    }

    #[test]
    fn zero_capacity_keeps_latest() {
        let mut log = AccessLog::with_capacity(0);
        log.record(AccessEvent::granted("a", DoorId::new("d"), Direction::Entry, 1));
        log.record(AccessEvent::granted("b", DoorId::new("d"), Direction::Entry, 2));
        assert_eq!(log.len(), 1);
        assert_eq!(log.last().expect("non-empty").who, "b");
    }

    #[test]
    fn granted_for_filters_person_and_outcome() {
        let mut log = AccessLog::new();
        log.record(AccessEvent::granted("alice", DoorId::new("d"), Direction::Entry, 1));
        log.record(AccessEvent::denied("alice", DoorId::new("d"), DenyReason::OutsideSchedule, 2));
        log.record(AccessEvent::granted("bob", DoorId::new("d"), Direction::Entry, 3));
        let alice = log.granted_for("alice");
        assert_eq!(alice.len(), 1);
        assert_eq!(alice[0].tick, 1);
    }

    #[test]
    fn passback_suspicious_on_double_entry() {
        let mut log = AccessLog::new();
        log.record(AccessEvent::granted("alice", DoorId::new("d"), Direction::Entry, 1));
        // Alice already entered and has not exited — entering again is suspicious.
        assert!(log.is_passback_suspicious("alice", Direction::Entry));
    }

    #[test]
    fn passback_not_suspicious_after_exit() {
        let mut log = AccessLog::new();
        log.record(AccessEvent::granted("alice", DoorId::new("d"), Direction::Entry, 1));
        log.record(AccessEvent::granted("alice", DoorId::new("d"), Direction::Exit, 2));
        // She left; a fresh entry is fine.
        assert!(!log.is_passback_suspicious("alice", Direction::Entry));
    }

    #[test]
    fn passback_ignores_exit_intent() {
        let mut log = AccessLog::new();
        log.record(AccessEvent::granted("alice", DoorId::new("d"), Direction::Entry, 1));
        // Leaving is never a passback concern.
        assert!(!log.is_passback_suspicious("alice", Direction::Exit));
    }

    #[test]
    fn passback_first_ever_entry_is_fine() {
        let log = AccessLog::new();
        assert!(!log.is_passback_suspicious("alice", Direction::Entry));
    }
}
