//! The visitor-log model: a small, append-only history of front-door
//! interactions the household can scroll back through.
//!
//! Each entry records *what* happened ([`DoorbellEvent`]), *when* (a
//! caller-supplied tick), and — for an entry that closed a visit — *how it
//! turned out* ([`CallState`]). The log itself is pure in-memory state; durable
//! storage and the Portal/mobile history view are Phase-1b surfaces (see
//! `parity.manifest.toml`).

use crate::event::{CallState, DoorbellEvent, Tick};

/// One line in the visitor history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisitorEntry {
    /// The event that produced this entry.
    pub event: DoorbellEvent,
    /// When it happened (whole seconds, caller's clock).
    pub at: Tick,
    /// The visit outcome this entry settled on, if it closed a visit. A bare
    /// ring/motion entry that is still in flight carries `None`.
    pub outcome: Option<CallState>,
}

impl VisitorEntry {
    /// A log entry for an in-flight event with no settled outcome yet.
    #[must_use]
    pub const fn observed(event: DoorbellEvent, at: Tick) -> Self {
        Self { event, at, outcome: None }
    }

    /// A log entry that records a visit's settled outcome.
    #[must_use]
    pub const fn settled(event: DoorbellEvent, at: Tick, outcome: CallState) -> Self {
        Self { event, at, outcome: Some(outcome) }
    }

    /// Whether this entry closed a visit (carries a terminal outcome).
    #[must_use]
    pub fn is_settled(&self) -> bool {
        self.outcome.is_some_and(CallState::is_terminal)
    }
}

/// An append-only visitor log holding the most recent entries up to a cap.
///
/// The cap keeps memory bounded on a long-running controller; the oldest entry
/// is dropped when a new one would exceed it. A cap of zero disables the log.
#[derive(Debug, Clone, Default)]
pub struct VisitorLog {
    entries: Vec<VisitorEntry>,
    cap: usize,
}

impl VisitorLog {
    /// A log that retains at most `cap` most-recent entries.
    #[must_use]
    pub const fn with_capacity(cap: usize) -> Self {
        Self { entries: Vec::new(), cap }
    }

    /// Append an entry, evicting the oldest if the cap would be exceeded.
    pub fn record(&mut self, entry: VisitorEntry) {
        if self.cap == 0 {
            return;
        }
        if self.entries.len() >= self.cap {
            self.entries.remove(0);
        }
        self.entries.push(entry);
    }

    /// The entries, oldest first.
    #[must_use]
    pub fn entries(&self) -> &[VisitorEntry] {
        &self.entries
    }

    /// The most recent entry, if any.
    #[must_use]
    pub fn latest(&self) -> Option<&VisitorEntry> {
        self.entries.last()
    }

    /// How many recorded visits ended as missed — the "did I miss anyone?"
    /// count the household most wants to see.
    #[must_use]
    pub fn missed_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.outcome == Some(CallState::Missed))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_entry_has_no_outcome() {
        let e = VisitorEntry::observed(DoorbellEvent::ButtonPressed, 5);
        assert_eq!(e.outcome, None);
        assert!(!e.is_settled());
    }

    #[test]
    fn settled_entry_carries_terminal_outcome() {
        let e = VisitorEntry::settled(DoorbellEvent::VisitorTimeout, 35, CallState::Missed);
        assert_eq!(e.outcome, Some(CallState::Missed));
        assert!(e.is_settled());
    }

    #[test]
    fn non_terminal_outcome_is_not_settled() {
        let e = VisitorEntry { event: DoorbellEvent::CallAnswered, at: 1, outcome: Some(CallState::Answered) };
        assert!(!e.is_settled(), "an in-progress call is not a settled visit");
    }

    #[test]
    fn log_keeps_entries_in_order() {
        let mut log = VisitorLog::with_capacity(8);
        log.record(VisitorEntry::observed(DoorbellEvent::ButtonPressed, 1));
        log.record(VisitorEntry::settled(DoorbellEvent::CallAnswered, 3, CallState::Answered));
        assert_eq!(log.entries().len(), 2);
        assert_eq!(log.entries()[0].at, 1);
        assert_eq!(log.latest().map(|e| e.at), Some(3));
    }

    #[test]
    fn log_evicts_oldest_past_cap() {
        let mut log = VisitorLog::with_capacity(2);
        log.record(VisitorEntry::observed(DoorbellEvent::ButtonPressed, 1));
        log.record(VisitorEntry::observed(DoorbellEvent::ButtonPressed, 2));
        log.record(VisitorEntry::observed(DoorbellEvent::ButtonPressed, 3));
        assert_eq!(log.entries().len(), 2);
        assert_eq!(log.entries()[0].at, 2, "oldest (t=1) was evicted");
        assert_eq!(log.entries()[1].at, 3);
    }

    #[test]
    fn zero_cap_records_nothing() {
        let mut log = VisitorLog::with_capacity(0);
        log.record(VisitorEntry::observed(DoorbellEvent::ButtonPressed, 1));
        assert!(log.entries().is_empty());
        assert_eq!(log.latest(), None);
    }

    #[test]
    fn missed_count_tallies_only_misses() {
        let mut log = VisitorLog::with_capacity(8);
        log.record(VisitorEntry::settled(DoorbellEvent::VisitorTimeout, 1, CallState::Missed));
        log.record(VisitorEntry::settled(DoorbellEvent::CallAnswered, 2, CallState::Ended));
        log.record(VisitorEntry::settled(DoorbellEvent::VisitorTimeout, 3, CallState::Missed));
        assert_eq!(log.missed_count(), 2);
    }
}
