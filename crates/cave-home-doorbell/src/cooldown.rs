//! Motion / ring de-duplication: collapse a burst of repeated door signals into
//! a single visit so the household is not spammed.
//!
//! A real PIR sensor or a jittery doorbell button can fire many times a second.
//! Without de-dup that becomes a flood of notifications and camera clips for one
//! visitor. This module is a pure function over a caller-supplied "last accepted
//! tick": within the cooldown window a repeat signal is suppressed; once the
//! window has elapsed the next signal is accepted and becomes the new anchor.
//!
//! The crate reads no clock — the caller passes the previous accepted tick and
//! the current tick in whole seconds (see [`crate::event::Tick`]).

use crate::event::Tick;

/// The de-dup verdict for one incoming door signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dedup {
    /// Accept this signal — it is the first, or the cooldown since the last
    /// accepted signal has elapsed. The carried tick is the new anchor the
    /// caller should remember.
    Accept(Tick),
    /// Suppress this signal — it falls within the cooldown of the last accepted
    /// one.
    Suppress,
}

impl Dedup {
    /// Whether this verdict accepts the signal.
    #[must_use]
    pub const fn is_accepted(self) -> bool {
        matches!(self, Self::Accept(_))
    }
}

/// Decide whether a door signal arriving at `now` should be accepted, given the
/// `last_accepted` tick (or `None` if none has been accepted yet) and a
/// `cooldown` of whole seconds.
///
/// A signal is accepted when there is no prior signal, or when at least
/// `cooldown` whole seconds have elapsed since the last accepted one. The
/// boundary is inclusive: a signal exactly `cooldown` seconds after the anchor
/// is accepted. A `cooldown` of zero accepts every signal.
///
/// The comparison is saturating, so a `now` earlier than `last_accepted` (a
/// caller handing back a non-monotonic clock) is treated as "no time elapsed"
/// and suppressed rather than panicking.
#[must_use]
pub fn dedup(last_accepted: Option<Tick>, now: Tick, cooldown: Tick) -> Dedup {
    match last_accepted {
        None => Dedup::Accept(now),
        Some(prev) => {
            if now.saturating_sub(prev) >= cooldown {
                Dedup::Accept(now)
            } else {
                Dedup::Suppress
            }
        }
    }
}

/// A small stateful helper around [`dedup`] for callers that would rather hold
/// the anchor than thread it through themselves. Still clock-free: the caller
/// supplies `now`.
#[derive(Debug, Clone, Copy, Default)]
pub struct CooldownGate {
    last_accepted: Option<Tick>,
}

impl CooldownGate {
    /// A fresh gate that has accepted nothing yet.
    #[must_use]
    pub const fn new() -> Self {
        Self { last_accepted: None }
    }

    /// The tick of the last accepted signal, if any.
    #[must_use]
    pub const fn last_accepted(&self) -> Option<Tick> {
        self.last_accepted
    }

    /// Offer a signal at `now`; advance the anchor and return `true` if it is
    /// accepted, `false` if suppressed.
    pub fn offer(&mut self, now: Tick, cooldown: Tick) -> bool {
        match dedup(self.last_accepted, now, cooldown) {
            Dedup::Accept(t) => {
                self.last_accepted = Some(t);
                true
            }
            Dedup::Suppress => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_signal_is_always_accepted() {
        assert_eq!(dedup(None, 100, 30), Dedup::Accept(100));
    }

    #[test]
    fn repeat_within_cooldown_is_suppressed() {
        // Last accepted at 100, cooldown 30 -> a signal at 120 is within window.
        assert_eq!(dedup(Some(100), 120, 30), Dedup::Suppress);
    }

    #[test]
    fn signal_exactly_at_cooldown_boundary_is_accepted() {
        // 100 + 30 == 130: inclusive boundary accepts.
        assert_eq!(dedup(Some(100), 130, 30), Dedup::Accept(130));
    }

    #[test]
    fn signal_one_second_before_boundary_is_suppressed() {
        assert_eq!(dedup(Some(100), 129, 30), Dedup::Suppress);
    }

    #[test]
    fn zero_cooldown_accepts_everything() {
        assert_eq!(dedup(Some(100), 100, 0), Dedup::Accept(100));
        assert_eq!(dedup(Some(100), 101, 0), Dedup::Accept(101));
    }

    #[test]
    fn non_monotonic_now_is_suppressed_not_panicking() {
        // now earlier than the anchor: saturating_sub -> 0 elapsed -> suppress.
        assert_eq!(dedup(Some(100), 50, 30), Dedup::Suppress);
    }

    #[test]
    fn gate_collapses_a_burst_then_reopens() {
        let mut g = CooldownGate::new();
        assert!(g.offer(0, 10), "first press accepted");
        assert!(!g.offer(3, 10), "burst repeat suppressed");
        assert!(!g.offer(9, 10), "still within cooldown");
        assert!(g.offer(10, 10), "cooldown elapsed, accepted");
        assert_eq!(g.last_accepted(), Some(10));
        assert!(!g.offer(15, 10), "new window from the re-accept");
    }

    #[test]
    fn is_accepted_predicate() {
        assert!(Dedup::Accept(5).is_accepted());
        assert!(!Dedup::Suppress.is_accepted());
    }
}
