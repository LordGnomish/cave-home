// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Grandma-friendly forecast summary — the public output type of
//! both Forecast.Solar and PVGIS clients.

use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// One hourly slot in the forecast.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ForecastSlot {
    pub start: SystemTime,
    /// Estimated energy in this slot, kWh.
    pub kwh: f64,
}

/// Forecast summary — what Portal / cavectl render to the user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Forecast {
    /// Source identifier — "forecast.solar" or "pvgis".
    pub source: &'static str,
    pub kwh_today: f64,
    pub kwh_tomorrow: f64,
    /// Peak instantaneous power forecast, kW.
    pub peak_kw: f64,
    /// Hourly slots in chronological order. May be empty if upstream
    /// only returns daily totals.
    pub hourly: Vec<ForecastSlot>,
}

impl Forecast {
    /// Build a fresh summary with no hourly slots.
    #[must_use]
    pub fn new(source: &'static str) -> Self {
        Self {
            source,
            kwh_today: 0.0,
            kwh_tomorrow: 0.0,
            peak_kw: 0.0,
            hourly: Vec::new(),
        }
    }

    /// Compute the sum of `hourly` slot energies into `kwh_today` /
    /// `kwh_tomorrow` and the peak across all slots. Helper for
    /// derived clients to keep aggregate fields consistent.
    pub fn recompute_aggregates(&mut self) {
        let mut today = 0.0;
        let mut tomorrow = 0.0;
        let mut peak = 0.0f64;
        let now = SystemTime::now();
        let twenty_four_h = std::time::Duration::from_secs(24 * 3600);
        let today_end = now + twenty_four_h;
        let tomorrow_end = now + twenty_four_h * 2;
        for slot in &self.hourly {
            if slot.start <= today_end {
                today += slot.kwh;
            } else if slot.start <= tomorrow_end {
                tomorrow += slot.kwh;
            }
            // Assume 1-hour slots ⇒ instantaneous power == kWh × 1 / 1h.
            if slot.kwh > peak {
                peak = slot.kwh;
            }
        }
        self.kwh_today = today;
        self.kwh_tomorrow = tomorrow;
        self.peak_kw = peak;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    #[test]
    fn new_starts_zeroed() {
        let f = Forecast::new("test");
        assert_eq!(f.kwh_today, 0.0);
        assert_eq!(f.kwh_tomorrow, 0.0);
        assert!(f.hourly.is_empty());
    }

    #[test]
    fn recompute_finds_peak() {
        let mut f = Forecast::new("test");
        let now = SystemTime::now();
        f.hourly = vec![
            ForecastSlot {
                start: now,
                kwh: 1.0,
            },
            ForecastSlot {
                start: now + Duration::from_secs(3600),
                kwh: 7.5,
            },
            ForecastSlot {
                start: now + Duration::from_secs(7200),
                kwh: 3.0,
            },
        ];
        f.recompute_aggregates();
        assert!((f.peak_kw - 7.5).abs() < f64::EPSILON);
        assert!((f.kwh_today - 11.5).abs() < f64::EPSILON);
    }

    #[test]
    fn recompute_splits_today_tomorrow() {
        let mut f = Forecast::new("test");
        let now = SystemTime::now();
        f.hourly = vec![
            ForecastSlot {
                start: now,
                kwh: 5.0,
            },
            ForecastSlot {
                start: now + Duration::from_secs(36 * 3600),
                kwh: 4.0,
            },
        ];
        f.recompute_aggregates();
        assert!((f.kwh_today - 5.0).abs() < f64::EPSILON);
        assert!((f.kwh_tomorrow - 4.0).abs() < f64::EPSILON);
    }
}
