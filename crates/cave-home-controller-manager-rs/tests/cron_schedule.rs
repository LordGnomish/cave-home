// SPDX-License-Identifier: Apache-2.0
//! Unit tests for the 5-field cron expression parser + next-fire computation
//! (`robfig/cron` standard-parser semantics, as used by `pkg/controller/cronjob`).

use cave_home_controller_manager_rs::schedule::{CronSchedule, CronError};

/// 2021-01-01T00:00:00Z — a known Friday — as epoch seconds.
const FRIDAY_2021: i64 = 1_609_459_200;

#[test]
fn every_minute_advances_to_the_next_minute() {
    let s = CronSchedule::parse("* * * * *").unwrap();
    // From exactly a minute boundary, the next fire is the following minute.
    assert_eq!(s.next_after(FRIDAY_2021), FRIDAY_2021 + 60);
    // From mid-minute, it rounds up to the next whole minute.
    assert_eq!(s.next_after(FRIDAY_2021 + 17), FRIDAY_2021 + 60);
}

#[test]
fn fixed_minute_each_hour() {
    // "30 * * * *" → minute 30 of every hour.
    let s = CronSchedule::parse("30 * * * *").unwrap();
    // From 00:00:00 the next fire is 00:30:00.
    assert_eq!(s.next_after(FRIDAY_2021), FRIDAY_2021 + 30 * 60);
    // From 00:30:00 the next fire is 01:30:00.
    let at_0030 = FRIDAY_2021 + 30 * 60;
    assert_eq!(s.next_after(at_0030), at_0030 + 60 * 60);
}

#[test]
fn daily_at_a_fixed_time() {
    // "0 3 * * *" → 03:00 every day.
    let s = CronSchedule::parse("0 3 * * *").unwrap();
    let three_am = FRIDAY_2021 + 3 * 3600;
    assert_eq!(s.next_after(FRIDAY_2021), three_am);
    // The day after 03:00 it advances 24h.
    assert_eq!(s.next_after(three_am), three_am + 86_400);
}

#[test]
fn step_values_in_the_minute_field() {
    // "*/15 * * * *" → every 15 minutes.
    let s = CronSchedule::parse("*/15 * * * *").unwrap();
    assert_eq!(s.next_after(FRIDAY_2021), FRIDAY_2021 + 15 * 60);
    assert_eq!(s.next_after(FRIDAY_2021 + 15 * 60), FRIDAY_2021 + 30 * 60);
    // After :45 the next is the top of the next hour (:00).
    assert_eq!(s.next_after(FRIDAY_2021 + 45 * 60), FRIDAY_2021 + 60 * 60);
}

#[test]
fn lists_and_ranges_in_the_hour_field() {
    // "0 9,17 * * 1-5" → 09:00 and 17:00 on weekdays (Mon-Fri).
    let s = CronSchedule::parse("0 9,17 * * 1-5").unwrap();
    // 2021-01-01 is a Friday: 09:00 then 17:00 are valid.
    let nine = FRIDAY_2021 + 9 * 3600;
    let seventeen = FRIDAY_2021 + 17 * 3600;
    assert_eq!(s.next_after(FRIDAY_2021), nine);
    assert_eq!(s.next_after(nine), seventeen);
    // After Friday 17:00 the next is Monday 09:00 (skips Sat+Sun).
    let monday_nine = FRIDAY_2021 + 3 * 86_400 + 9 * 3600;
    assert_eq!(s.next_after(seventeen), monday_nine);
}

#[test]
fn day_of_week_and_day_of_month_are_or_ed_when_both_restricted() {
    // robfig semantics: if BOTH dom and dow are restricted, a day matches when
    // EITHER matches. "0 0 13 * 5" → midnight on the 13th OR any Friday.
    let s = CronSchedule::parse("0 0 13 * 5").unwrap();
    // From Fri 2021-01-01 00:00:00, next is the following Friday (Jan 8) 00:00,
    // which comes before the 13th.
    let jan8 = FRIDAY_2021 + 7 * 86_400;
    assert_eq!(s.next_after(FRIDAY_2021), jan8);
}

#[test]
fn sunday_accepts_both_zero_and_seven() {
    let z = CronSchedule::parse("0 0 * * 0").unwrap();
    let seven = CronSchedule::parse("0 0 * * 7").unwrap();
    // 2021-01-03 is the first Sunday after Jan 1.
    let sunday = FRIDAY_2021 + 2 * 86_400;
    assert_eq!(z.next_after(FRIDAY_2021), sunday);
    assert_eq!(seven.next_after(FRIDAY_2021), sunday);
}

#[test]
fn month_rollover_into_the_next_year() {
    // "0 0 1 1 *" → midnight Jan 1 every year.
    let s = CronSchedule::parse("0 0 1 1 *").unwrap();
    // From 2021-01-01 00:00:00 the next fire is 2022-01-01 00:00:00.
    let jan1_2022 = 1_640_995_200;
    assert_eq!(s.next_after(FRIDAY_2021), jan1_2022);
}

#[test]
fn leap_year_february_29_is_reachable() {
    // "0 0 29 2 *" → Feb 29, only in leap years. From 2021 the next is 2024.
    let s = CronSchedule::parse("0 0 29 2 *").unwrap();
    let feb29_2024 = 1_709_164_800; // 2024-02-29T00:00:00Z
    assert_eq!(s.next_after(FRIDAY_2021), feb29_2024);
}

#[test]
fn rejects_wrong_field_count() {
    assert!(matches!(CronSchedule::parse("* * * *"), Err(CronError::FieldCount(4))));
    assert!(matches!(CronSchedule::parse("* * * * * *"), Err(CronError::FieldCount(6))));
}

#[test]
fn rejects_out_of_range_values() {
    assert!(CronSchedule::parse("60 * * * *").is_err(), "minute 60 is out of range");
    assert!(CronSchedule::parse("* 24 * * *").is_err(), "hour 24 is out of range");
    assert!(CronSchedule::parse("* * 0 * *").is_err(), "day-of-month 0 is out of range");
    assert!(CronSchedule::parse("* * * 13 *").is_err(), "month 13 is out of range");
    assert!(CronSchedule::parse("* * * * 8").is_err(), "day-of-week 8 is out of range");
}

#[test]
fn rejects_inverted_ranges_and_garbage() {
    assert!(CronSchedule::parse("5-3 * * * *").is_err(), "inverted range");
    assert!(CronSchedule::parse("x * * * *").is_err(), "non-numeric");
    assert!(CronSchedule::parse("*/0 * * * *").is_err(), "zero step");
}
