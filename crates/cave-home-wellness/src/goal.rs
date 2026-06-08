//! Personal wellness goals and progress.
//!
//! A [`Goal`] pairs a [`GoalMetric`] (what to count) with a target and a
//! [`Period`] (how often it resets). [`progress`] measures one day's
//! [`DailyMetrics`] against a goal and returns a [`GoalProgress`] with a clamped
//! percent (0..=100) and a `met` flag. All pure — no clock, no I/O.

use crate::metric::DailyMetrics;

/// Which metric a goal tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalMetric {
    /// Daily step count.
    Steps,
    /// Daily moderate-to-vigorous active minutes.
    ActiveMinutes,
    /// Nightly sleep, measured in minutes.
    SleepMinutes,
}

/// How often a goal resets. The engine is day-tick based; the period is
/// metadata the caller uses to decide which days to feed in (this crate never
/// reads a clock).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    /// Resets each day.
    Daily,
    /// Resets each week.
    Weekly,
}

/// A wellness goal: a target amount of a metric over a period.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Goal {
    /// Which metric this goal tracks.
    pub metric: GoalMetric,
    /// The target amount (steps, minutes — matching `metric`'s unit).
    pub target: u32,
    /// How often the goal resets.
    pub period: Period,
}

impl Goal {
    /// A daily step goal (e.g. the classic 10,000 steps).
    #[must_use]
    pub const fn daily_steps(target: u32) -> Self {
        Self {
            metric: GoalMetric::Steps,
            target,
            period: Period::Daily,
        }
    }

    /// A daily active-minutes goal (e.g. 30 minutes of movement).
    #[must_use]
    pub const fn daily_active_minutes(target: u32) -> Self {
        Self {
            metric: GoalMetric::ActiveMinutes,
            target,
            period: Period::Daily,
        }
    }

    /// A nightly sleep goal expressed in minutes (e.g. 480 = 8 hours).
    #[must_use]
    pub const fn nightly_sleep_minutes(target: u32) -> Self {
        Self {
            metric: GoalMetric::SleepMinutes,
            target,
            period: Period::Daily,
        }
    }

    /// The value this goal reads out of a day's metrics.
    #[must_use]
    fn observed(self, metrics: &DailyMetrics) -> u32 {
        match self.metric {
            GoalMetric::Steps => metrics.steps.get(),
            GoalMetric::ActiveMinutes => u32::from(metrics.active.get()),
            GoalMetric::SleepMinutes => u32::from(metrics.sleep.minutes()),
        }
    }
}

/// The result of measuring a day against a goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoalProgress {
    /// Progress toward the target, clamped to 0..=100.
    pub percent: u8,
    /// Whether the observed value reached (or exceeded) the target.
    pub met: bool,
}

/// Measure one day's [`DailyMetrics`] against a [`Goal`].
///
/// A zero target is treated as already met (100%). Otherwise the percent is the
/// observed value over the target, clamped to 100; `met` is true once the
/// observed value reaches the target.
#[must_use]
pub fn progress(metrics: &DailyMetrics, goal: Goal) -> GoalProgress {
    let observed = goal.observed(metrics);
    if goal.target == 0 {
        return GoalProgress {
            percent: 100,
            met: true,
        };
    }
    let met = observed >= goal.target;
    // Compute in u64 to avoid overflow, clamp into 0..=100 for the u8 percent.
    let raw = (u64::from(observed) * 100) / u64::from(goal.target);
    let percent = u8::try_from(raw.min(100)).unwrap_or(100);
    GoalProgress { percent, met }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metric::{ActiveMinutes, HeartRate, SleepDuration, Steps};

    fn day(steps: u32, active: u16, sleep_min: u16) -> DailyMetrics {
        DailyMetrics::new(
            1,
            Steps::new(steps).expect("test step count in range"),
            ActiveMinutes::new(active).expect("test active minutes in range"),
            SleepDuration::from_minutes(sleep_min).expect("test sleep minutes in range"),
            HeartRate::new(60).expect("test heart rate in range"),
        )
    }

    #[test]
    fn step_goal_progress_and_met() {
        let g = Goal::daily_steps(10_000);
        let p = progress(&day(5_000, 0, 0), g);
        assert_eq!(p.percent, 50);
        assert!(!p.met);
    }

    #[test]
    fn step_goal_met_at_exact_boundary() {
        let g = Goal::daily_steps(10_000);
        let p = progress(&day(10_000, 0, 0), g);
        assert_eq!(p.percent, 100);
        assert!(p.met);
    }

    #[test]
    fn step_goal_just_under_boundary_not_met() {
        let g = Goal::daily_steps(10_000);
        let p = progress(&day(9_999, 0, 0), g);
        assert_eq!(p.percent, 99);
        assert!(!p.met);
    }

    #[test]
    fn over_target_clamps_percent_but_stays_met() {
        let g = Goal::daily_steps(10_000);
        let p = progress(&day(25_000, 0, 0), g);
        assert_eq!(p.percent, 100);
        assert!(p.met);
    }

    #[test]
    fn active_minutes_goal() {
        let g = Goal::daily_active_minutes(30);
        let p = progress(&day(0, 15, 0), g);
        assert_eq!(p.percent, 50);
        assert!(!p.met);
        let p2 = progress(&day(0, 30, 0), g);
        assert!(p2.met);
    }

    #[test]
    fn sleep_goal_in_minutes() {
        let g = Goal::nightly_sleep_minutes(480);
        let p = progress(&day(0, 0, 480), g);
        assert!(p.met);
        assert_eq!(p.percent, 100);
        let short = progress(&day(0, 0, 360), g);
        assert_eq!(short.percent, 75);
        assert!(!short.met);
    }

    #[test]
    fn zero_target_is_already_met() {
        let g = Goal::daily_steps(0);
        let p = progress(&day(0, 0, 0), g);
        assert!(p.met);
        assert_eq!(p.percent, 100);
    }
}
