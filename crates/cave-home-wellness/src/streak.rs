//! Goal-streak counting over a caller-supplied series of days.
//!
//! All pure: the caller passes in a chronological series of [`DailyMetrics`] and
//! a [`Goal`], and the streak functions report how many days in a row the goal
//! was met. No clock, no storage — the persistent on-device history store that
//! feeds these is deferred to phase-1b (see the parity manifest).

use crate::goal::{progress, Goal};
use crate::metric::DailyMetrics;

/// The longest run of consecutive goal-met days anywhere in the series.
///
/// The series is taken in the order given; an empty series has a streak of 0.
#[must_use]
pub fn longest_streak(days: &[DailyMetrics], goal: Goal) -> u32 {
    let mut best = 0u32;
    let mut run = 0u32;
    for day in days {
        if progress(day, goal).met {
            run += 1;
            if run > best {
                best = run;
            }
        } else {
            run = 0;
        }
    }
    best
}

/// The current streak: consecutive goal-met days counting back from the most
/// recent (last) day in the series.
///
/// The series is assumed chronological (oldest first). A series whose last day
/// missed the goal has a current streak of 0.
#[must_use]
pub fn current_streak(days: &[DailyMetrics], goal: Goal) -> u32 {
    let mut run = 0u32;
    for day in days.iter().rev() {
        if progress(day, goal).met {
            run += 1;
        } else {
            break;
        }
    }
    run
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metric::{ActiveMinutes, HeartRate, SleepDuration, Steps};

    fn day_with_steps(day: u32, steps: u32) -> DailyMetrics {
        DailyMetrics::new(
            day,
            Steps::new(steps).expect("test step count in range"),
            ActiveMinutes::new(0).expect("zero active minutes"),
            SleepDuration::from_minutes(0).expect("zero sleep"),
            HeartRate::new(60).expect("test heart rate"),
        )
    }

    fn series(step_days: &[u32]) -> Vec<DailyMetrics> {
        step_days
            .iter()
            .enumerate()
            .map(|(i, &s)| day_with_steps(u32::try_from(i).unwrap_or(0), s))
            .collect()
    }

    #[test]
    fn empty_series_has_no_streak() {
        let g = Goal::daily_steps(10_000);
        assert_eq!(longest_streak(&[], g), 0);
        assert_eq!(current_streak(&[], g), 0);
    }

    #[test]
    fn all_met_is_full_length_streak() {
        let g = Goal::daily_steps(10_000);
        let days = series(&[10_000, 11_000, 12_000, 10_500]);
        assert_eq!(longest_streak(&days, g), 4);
        assert_eq!(current_streak(&days, g), 4);
    }

    #[test]
    fn longest_streak_finds_best_run_with_gaps() {
        let g = Goal::daily_steps(10_000);
        // met, met, MISS, met, met, met, MISS
        let days = series(&[10_000, 10_000, 3_000, 10_000, 10_000, 10_000, 2_000]);
        assert_eq!(longest_streak(&days, g), 3);
    }

    #[test]
    fn current_streak_counts_back_from_last_day() {
        let g = Goal::daily_steps(10_000);
        // ends on two met days after a miss
        let days = series(&[10_000, 3_000, 10_000, 10_000]);
        assert_eq!(current_streak(&days, g), 2);
        assert_eq!(longest_streak(&days, g), 2);
    }

    #[test]
    fn current_streak_zero_when_last_day_misses() {
        let g = Goal::daily_steps(10_000);
        let days = series(&[10_000, 10_000, 4_000]);
        assert_eq!(current_streak(&days, g), 0);
        assert_eq!(longest_streak(&days, g), 2);
    }
}
