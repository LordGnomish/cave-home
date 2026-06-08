//! Typed-state history: the non-numeric companion to the numeric engine.
//!
//! Plenty of household history is not a number on a curve — it is *what state
//! something was in and for how long*: a light "on" / "off", a person "home" /
//! "away", a door "open" / "closed". A [`StateSample`] records "at this time,
//! the state became X". A [`StateTimeline`] is the ordered run of those
//! changes, and [`StateTimeline::durations`] answers the question a household
//! actually asks: *"how long was the light on today?"*
//!
//! Duration is measured between consecutive changes; the final, still-current
//! state is measured up to a caller-supplied `until` (typically "now"). No
//! clock here — `until` comes from the caller, like everywhere else.

use std::collections::BTreeMap;

/// One state change: at `timestamp`, the thing entered `state`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateSample {
    timestamp: i64,
    state: String,
}

impl StateSample {
    /// Record that `state` began at `timestamp`.
    #[must_use]
    pub fn new(timestamp: i64, state: impl Into<String>) -> Self {
        Self { timestamp, state: state.into() }
    }

    /// When this state began.
    #[must_use]
    pub const fn timestamp(&self) -> i64 {
        self.timestamp
    }

    /// The state entered.
    #[must_use]
    pub fn state(&self) -> &str {
        &self.state
    }
}

/// An ordered run of state changes for one thing.
///
/// Consecutive duplicate states are collapsed on construction: a sensor that
/// re-reports "on, on, on" describes one continuous "on", so only the first
/// timestamp is kept. This makes [`StateTimeline::durations`] count real time-
/// in-state rather than reporting noise.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StateTimeline {
    changes: Vec<StateSample>,
}

impl StateTimeline {
    /// Build from state changes in any order. They are sorted by timestamp
    /// (stable) and consecutive duplicate states are coalesced.
    #[must_use]
    pub fn new(mut changes: Vec<StateSample>) -> Self {
        changes.sort_by_key(StateSample::timestamp);
        let mut coalesced: Vec<StateSample> = Vec::with_capacity(changes.len());
        for change in changes {
            match coalesced.last() {
                Some(prev) if prev.state() == change.state() => {}
                _ => coalesced.push(change),
            }
        }
        Self { changes: coalesced }
    }

    /// The changes, time-ordered and coalesced.
    #[must_use]
    pub fn changes(&self) -> &[StateSample] {
        &self.changes
    }

    /// Whether the timeline is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// The current (latest) state at or before `until`, if any.
    #[must_use]
    pub fn current_state(&self, until: i64) -> Option<&str> {
        self.changes
            .iter()
            .rev()
            .find(|c| c.timestamp() <= until)
            .map(StateSample::state)
    }

    /// Total time spent in each state between the first change and `until`,
    /// keyed by state name.
    ///
    /// Each segment runs from one change to the next; the last segment runs to
    /// `until`. Changes at or after `until`, and the time *before* the first
    /// change, contribute nothing. A non-positive segment (e.g. `until` before
    /// the first change) is ignored.
    #[must_use]
    pub fn durations(&self, until: i64) -> BTreeMap<String, i64> {
        let mut totals: BTreeMap<String, i64> = BTreeMap::new();
        let n = self.changes.len();
        for (i, change) in self.changes.iter().enumerate() {
            let start = change.timestamp();
            if start >= until {
                break;
            }
            let end = if i + 1 < n {
                self.changes[i + 1].timestamp().min(until)
            } else {
                until
            };
            let dwell = end - start;
            if dwell > 0 {
                *totals.entry(change.state().to_string()).or_insert(0) += dwell;
            }
        }
        totals
    }

    /// Time spent in one specific state up to `until`. Convenience over
    /// [`StateTimeline::durations`] for the common "how long was it on?" query.
    #[must_use]
    pub fn duration_in(&self, state: &str, until: i64) -> i64 {
        self.durations(until).get(state).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesces_consecutive_duplicate_states() {
        let tl = StateTimeline::new(vec![
            StateSample::new(0, "on"),
            StateSample::new(10, "on"),
            StateSample::new(20, "off"),
        ]);
        assert_eq!(tl.changes().len(), 2);
        assert_eq!(tl.changes()[0].state(), "on");
        assert_eq!(tl.changes()[1].state(), "off");
    }

    #[test]
    fn sorts_out_of_order_changes() {
        let tl = StateTimeline::new(vec![
            StateSample::new(20, "off"),
            StateSample::new(0, "on"),
        ]);
        assert_eq!(tl.changes()[0].timestamp(), 0);
        assert_eq!(tl.changes()[1].timestamp(), 20);
    }

    #[test]
    fn duration_in_state_simple_on_off() {
        // on at 0, off at 3600, until 3600 -> on for 1 hour, off for 0.
        let tl = StateTimeline::new(vec![
            StateSample::new(0, "on"),
            StateSample::new(3600, "off"),
        ]);
        assert_eq!(tl.duration_in("on", 3600), 3600);
        assert_eq!(tl.duration_in("off", 3600), 0);
    }

    #[test]
    fn duration_counts_current_state_up_to_until() {
        // on at 0, off at 100, on at 300, "now" = 400.
        // on: (100-0) + (400-300) = 200; off: (300-100) = 200.
        let tl = StateTimeline::new(vec![
            StateSample::new(0, "on"),
            StateSample::new(100, "off"),
            StateSample::new(300, "on"),
        ]);
        let d = tl.durations(400);
        assert_eq!(d.get("on"), Some(&200));
        assert_eq!(d.get("off"), Some(&200));
    }

    #[test]
    fn presence_home_away() {
        // "home" from 0..480 (8h), "away" 480..1440 (until end of day).
        let tl = StateTimeline::new(vec![
            StateSample::new(0, "home"),
            StateSample::new(480, "away"),
        ]);
        assert_eq!(tl.duration_in("home", 1440), 480);
        assert_eq!(tl.duration_in("away", 1440), 960);
    }

    #[test]
    fn current_state_at_a_point_in_time() {
        let tl = StateTimeline::new(vec![
            StateSample::new(0, "on"),
            StateSample::new(100, "off"),
        ]);
        assert_eq!(tl.current_state(50), Some("on"));
        assert_eq!(tl.current_state(150), Some("off"));
        assert_eq!(tl.current_state(-1), None);
    }

    #[test]
    fn changes_after_until_are_ignored() {
        let tl = StateTimeline::new(vec![
            StateSample::new(0, "on"),
            StateSample::new(1000, "off"),
        ]);
        // until = 500, before the "off" change -> all 500 is "on".
        assert_eq!(tl.duration_in("on", 500), 500);
        assert_eq!(tl.duration_in("off", 500), 0);
    }

    #[test]
    fn empty_timeline_has_no_durations() {
        let tl = StateTimeline::default();
        assert!(tl.is_empty());
        assert!(tl.durations(1000).is_empty());
        assert_eq!(tl.duration_in("on", 1000), 0);
        assert_eq!(tl.current_state(1000), None);
    }

    #[test]
    fn until_before_first_change_yields_nothing() {
        let tl = StateTimeline::new(vec![StateSample::new(100, "on")]);
        assert!(tl.durations(50).is_empty());
    }
}
