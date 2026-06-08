//! The call-history model: a bounded, append-only list of past calls the
//! household can scroll back through ("missed call from the gate at lunch").
//!
//! Each [`CallRecord`] records who the call was *from*, who it was *to*, the
//! [`CallDirection`], how it turned out ([`crate::call::CallState`]), how long
//! it lasted, and the tick it started at. The log itself is pure in-memory
//! state; durable storage and the Portal history view are Phase-1b surfaces
//! (see `parity.manifest.toml`).

use crate::call::{CallState, Tick};

/// Whether a call came in or went out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallDirection {
    /// A call the household received.
    Incoming,
    /// A call the household placed.
    Outgoing,
}

/// One line in the call history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallRecord {
    /// Who the call was from (a number or a friendly name like "Front door").
    pub from: String,
    /// Who the call was to.
    pub to: String,
    /// Whether it was an incoming or outgoing call.
    pub direction: CallDirection,
    /// How the call ended.
    pub outcome: CallState,
    /// Talk time in whole seconds (`0` for a call that never connected).
    pub duration: Tick,
    /// The tick the call started ringing.
    pub started_at: Tick,
}

impl CallRecord {
    /// A record for a call that connected and lasted `duration` seconds.
    #[must_use]
    pub fn answered(
        from: impl Into<String>,
        to: impl Into<String>,
        direction: CallDirection,
        duration: Tick,
        started_at: Tick,
    ) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            direction,
            outcome: CallState::Ended,
            duration,
            started_at,
        }
    }

    /// A record for a call that rang out and never connected. `outcome` is
    /// [`CallState::Missed`] or [`CallState::Voicemail`]; duration is zero.
    #[must_use]
    pub fn unanswered(
        from: impl Into<String>,
        to: impl Into<String>,
        direction: CallDirection,
        outcome: CallState,
        started_at: Tick,
    ) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            direction,
            outcome,
            duration: 0,
            started_at,
        }
    }

    /// Whether this record represents a call the household missed (rang out
    /// with no answer and no voicemail).
    #[must_use]
    pub const fn is_missed(&self) -> bool {
        matches!(self.outcome, CallState::Missed)
    }
}

/// A bounded, append-only call log holding the most recent records up to a cap.
///
/// The cap keeps memory bounded on a long-running controller; the oldest record
/// is dropped when a new one would exceed it. A cap of zero disables the log.
#[derive(Debug, Clone, Default)]
pub struct CallLog {
    records: Vec<CallRecord>,
    cap: usize,
}

impl CallLog {
    /// A log that retains at most `cap` most-recent records.
    #[must_use]
    pub const fn with_capacity(cap: usize) -> Self {
        Self { records: Vec::new(), cap }
    }

    /// Append a record, evicting the oldest if the cap would be exceeded.
    pub fn record(&mut self, record: CallRecord) {
        if self.cap == 0 {
            return;
        }
        if self.records.len() >= self.cap {
            self.records.remove(0);
        }
        self.records.push(record);
    }

    /// The records, oldest first.
    #[must_use]
    pub fn records(&self) -> &[CallRecord] {
        &self.records
    }

    /// The most recent record, if any.
    #[must_use]
    pub fn latest(&self) -> Option<&CallRecord> {
        self.records.last()
    }

    /// How many recorded calls were missed — the "did I miss anyone?" count.
    #[must_use]
    pub fn missed_count(&self) -> usize {
        self.records.iter().filter(|r| r.is_missed()).count()
    }

    /// Total talk time across all recorded calls, in whole seconds.
    #[must_use]
    pub fn total_talk_time(&self) -> Tick {
        self.records.iter().map(|r| r.duration).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answered_record_is_not_missed() {
        let r = CallRecord::answered("Gate", "101", CallDirection::Incoming, 42, 100);
        assert_eq!(r.outcome, CallState::Ended);
        assert_eq!(r.duration, 42);
        assert!(!r.is_missed());
    }

    #[test]
    fn unanswered_missed_record() {
        let r = CallRecord::unanswered(
            "Gate",
            "101",
            CallDirection::Incoming,
            CallState::Missed,
            100,
        );
        assert!(r.is_missed());
        assert_eq!(r.duration, 0);
    }

    #[test]
    fn voicemail_record_is_not_counted_as_missed() {
        let r = CallRecord::unanswered(
            "Gate",
            "101",
            CallDirection::Incoming,
            CallState::Voicemail,
            100,
        );
        assert!(!r.is_missed(), "a voicemail was answered by the machine, not missed");
    }

    #[test]
    fn log_keeps_records_in_order() {
        let mut log = CallLog::with_capacity(8);
        log.record(CallRecord::unanswered(
            "Gate", "101", CallDirection::Incoming, CallState::Missed, 1,
        ));
        log.record(CallRecord::answered("101", "Kitchen", CallDirection::Outgoing, 10, 3));
        assert_eq!(log.records().len(), 2);
        assert_eq!(log.records()[0].started_at, 1);
        assert_eq!(log.latest().map(|r| r.started_at), Some(3));
    }

    #[test]
    fn log_evicts_oldest_past_cap() {
        let mut log = CallLog::with_capacity(2);
        for t in 1..=3 {
            log.record(CallRecord::answered("a", "b", CallDirection::Incoming, 1, t));
        }
        assert_eq!(log.records().len(), 2);
        assert_eq!(log.records()[0].started_at, 2, "oldest (t=1) evicted");
    }

    #[test]
    fn zero_cap_records_nothing() {
        let mut log = CallLog::with_capacity(0);
        log.record(CallRecord::answered("a", "b", CallDirection::Incoming, 1, 1));
        assert!(log.records().is_empty());
        assert_eq!(log.latest(), None);
    }

    #[test]
    fn missed_count_and_total_talk_time() {
        let mut log = CallLog::with_capacity(8);
        log.record(CallRecord::unanswered(
            "Gate", "101", CallDirection::Incoming, CallState::Missed, 1,
        ));
        log.record(CallRecord::answered("101", "102", CallDirection::Outgoing, 30, 2));
        log.record(CallRecord::answered("Gate", "101", CallDirection::Incoming, 12, 3));
        log.record(CallRecord::unanswered(
            "Gate", "101", CallDirection::Incoming, CallState::Missed, 4,
        ));
        assert_eq!(log.missed_count(), 2);
        assert_eq!(log.total_talk_time(), 42);
    }
}
