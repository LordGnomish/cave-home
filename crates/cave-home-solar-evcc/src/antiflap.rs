//! Anti-flap timer — don't start or stop the car on a passing cloud.
//!
//! Solar surplus is jittery: a cloud drops production for ten seconds, then
//! it's back. If the engine started and stopped the car on every wobble it
//! would wear the contactor and annoy the household. So a *condition* (enough
//! sun to charge, or too little to continue) must hold continuously for a
//! caller-supplied dwell time before the engine acts on it.
//!
//! The engine keeps no clock of its own (Charter §7: no hidden time): the
//! caller passes the **seconds elapsed** since the previous update, and the
//! timer accumulates them while the condition holds, resetting to zero the
//! moment it flips.

/// Accumulates how long a boolean condition has held, and reports when it has
/// held long enough to act on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AntiFlapTimer {
    required_s: f64,
    /// How long the current condition value has held, in seconds.
    held_s: f64,
    /// The condition value currently being timed (`None` until first update).
    current: Option<bool>,
}

impl AntiFlapTimer {
    /// A timer that requires a condition to hold for `required_s` seconds.
    ///
    /// A negative or non-finite `required_s` is treated as `0.0` (act
    /// immediately) so the timer can never wedge.
    #[must_use]
    pub fn new(required_s: f64) -> Self {
        let required_s = if required_s.is_finite() && required_s > 0.0 {
            required_s
        } else {
            0.0
        };
        Self { required_s, held_s: 0.0, current: None }
    }

    /// Feed an observation: `condition` is the present truth (e.g. "there is
    /// enough sun"), `elapsed_s` is the time since the last update.
    ///
    /// Returns `true` once `condition` has held continuously for at least the
    /// required dwell time. A non-finite or negative `elapsed_s` contributes
    /// no time (the observation still registers the condition value).
    pub fn update(&mut self, condition: bool, elapsed_s: f64) -> bool {
        let step = if elapsed_s.is_finite() && elapsed_s > 0.0 {
            elapsed_s
        } else {
            0.0
        };

        match self.current {
            Some(prev) if prev == condition => {
                self.held_s += step;
            }
            _ => {
                // First observation or the condition flipped — restart the dwell.
                self.current = Some(condition);
                self.held_s = step;
            }
        }

        self.is_satisfied()
    }

    /// Whether the currently-timed condition has held long enough.
    ///
    /// Returns `false` before any observation has been fed.
    #[must_use]
    pub fn is_satisfied(&self) -> bool {
        self.current.is_some() && self.held_s >= self.required_s
    }

    /// The condition value currently being timed, if any.
    #[must_use]
    pub const fn current(&self) -> Option<bool> {
        self.current
    }

    /// How long the present condition has held, in seconds.
    #[must_use]
    pub const fn held_seconds(&self) -> f64 {
        self.held_s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_satisfied_before_dwell_completes() {
        let mut t = AntiFlapTimer::new(60.0);
        assert!(!t.update(true, 30.0));
        assert!(!t.is_satisfied());
    }

    #[test]
    fn satisfied_exactly_at_dwell_boundary() {
        let mut t = AntiFlapTimer::new(60.0);
        assert!(!t.update(true, 59.0));
        assert!(t.update(true, 1.0)); // total 60.0 == required
    }

    #[test]
    fn flip_resets_the_dwell() {
        let mut t = AntiFlapTimer::new(60.0);
        t.update(true, 40.0);
        // A cloud: condition flips to false, then back to true.
        assert!(!t.update(false, 5.0));
        assert!(!t.update(true, 40.0)); // restarted, only 40s of the new run
        assert_eq!(t.held_seconds(), 40.0);
    }

    #[test]
    fn stays_satisfied_while_condition_holds() {
        let mut t = AntiFlapTimer::new(10.0);
        t.update(true, 10.0);
        assert!(t.update(true, 5.0)); // 15s, still holding
    }

    #[test]
    fn zero_dwell_acts_immediately() {
        let mut t = AntiFlapTimer::new(0.0);
        assert!(t.update(true, 0.0));
    }

    #[test]
    fn negative_required_is_clamped_to_zero() {
        let mut t = AntiFlapTimer::new(-5.0);
        assert!(t.update(true, 0.0));
    }

    #[test]
    fn non_finite_elapsed_contributes_no_time() {
        let mut t = AntiFlapTimer::new(10.0);
        assert!(!t.update(true, f64::NAN));
        assert_eq!(t.held_seconds(), 0.0);
        assert!(!t.update(true, f64::INFINITY));
        assert_eq!(t.held_seconds(), 0.0);
    }

    #[test]
    fn unfed_timer_is_not_satisfied() {
        let t = AntiFlapTimer::new(0.0);
        assert!(!t.is_satisfied());
        assert_eq!(t.current(), None);
    }
}
