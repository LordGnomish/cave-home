//! EPA NowCast (time-weighted PM averaging) — behavioural tests.
//!
//! Implemented from the public EPA NowCast definition (the same EPA-454/B-24-002
//! family already cited by the AQI engine). NowCast weights the most recent
//! hours more heavily when concentrations are changing fast, so the live tile
//! reacts quickly without a full 24-hour average.
//!
//! These tests are written FIRST (strict-TDD RED): they reference
//! `cave_home_air_quality::nowcast::now_cast`, which does not exist yet, so the
//! crate's test build fails until the implementation lands.

use cave_home_air_quality::nowcast::now_cast;

/// Hourly readings are most-recent-first; `None` is a missing hour.
fn h(values: &[Option<f64>]) -> Vec<Option<f64>> {
    values.to_vec()
}

#[test]
fn constant_concentration_is_itself() {
    // All weights equal (min/max = 1) -> plain average -> 20.0.
    let r = now_cast(&h(&[Some(20.0); 12])).expect("enough recent data");
    assert!((r - 20.0).abs() < 0.05, "got {r}");
}

#[test]
fn weight_floor_half_applies_when_range_is_large() {
    // [50, 10]: min/max = 0.2 < 0.5 -> weight floored to 0.5.
    // NowCast = (0.5^0*50 + 0.5^1*10) / (0.5^0 + 0.5^1) = 55/1.5 = 36.67.
    let r = now_cast(&h(&[Some(50.0), Some(10.0)])).expect("two recent hours");
    assert!((r - 36.6667).abs() < 0.01, "got {r}");
}

#[test]
fn rising_concentration_weights_recent_hours_more() {
    // [30,20,10]: min/max = 1/3 < 0.5 -> weight 0.5.
    // (30 + 0.5*20 + 0.25*10) / (1 + 0.5 + 0.25) = 42.5/1.75 = 24.2857.
    let r = now_cast(&h(&[Some(30.0), Some(20.0), Some(10.0)])).expect("data");
    assert!((r - 24.2857).abs() < 0.01, "got {r}");
}

#[test]
fn insufficient_recent_data_is_none() {
    // Only 1 of the 3 most-recent hours present -> NowCast unavailable.
    assert_eq!(now_cast(&h(&[None, None, Some(20.0), Some(20.0)])), None);
}

#[test]
fn two_of_three_recent_hours_is_enough() {
    assert!(now_cast(&h(&[Some(10.0), None, Some(10.0)])).is_some());
}

#[test]
fn no_data_is_none() {
    assert_eq!(now_cast(&h(&[None; 12])), None);
    assert_eq!(now_cast(&[]), None);
}

#[test]
fn all_zero_is_zero_not_nan() {
    let r = now_cast(&h(&[Some(0.0), Some(0.0), Some(0.0)])).expect("present");
    assert!((r - 0.0).abs() < 1e-9, "got {r}");
}
