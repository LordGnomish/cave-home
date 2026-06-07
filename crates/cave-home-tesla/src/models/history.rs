// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! A time range and the historical power series over it.

use crate::error::{Result, TeslaError};
use crate::fleet_api::types::HistorySeries;

/// A closed time range, in Unix seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateRange {
    /// Inclusive start, Unix seconds.
    pub start_unix: u64,
    /// Inclusive end, Unix seconds.
    pub end_unix: u64,
}

impl DateRange {
    /// A range from `start_unix` to `end_unix`.
    ///
    /// # Errors
    /// [`TeslaError::Validation`] if `end_unix < start_unix`.
    pub fn new(start_unix: u64, end_unix: u64) -> Result<Self> {
        if end_unix < start_unix {
            return Err(TeslaError::Validation(
                "history range end precedes start".into(),
            ));
        }
        Ok(Self { start_unix, end_unix })
    }

    /// The last `hours` ending at `now_unix`.
    #[must_use]
    pub const fn last_hours(now_unix: u64, hours: u64) -> Self {
        Self {
            start_unix: now_unix.saturating_sub(hours.saturating_mul(3_600)),
            end_unix: now_unix,
        }
    }

    /// The span of the range in seconds.
    #[must_use]
    pub const fn duration_secs(&self) -> u64 {
        self.end_unix.saturating_sub(self.start_unix)
    }
}

/// One historical power sample.
#[derive(Debug, Clone, PartialEq)]
pub struct HistorySample {
    /// ISO-8601 timestamp.
    pub timestamp: String,
    /// Solar production at the sample, watts.
    pub pv_watts: f64,
    /// Battery power at the sample, watts (negative = charging).
    pub battery_watts: f64,
    /// Grid power at the sample, watts (negative = exporting).
    pub grid_watts: f64,
}

/// A historical power series over a range.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryData {
    /// The aggregation period (`day`, `week`, …).
    pub period: String,
    /// The samples, oldest first.
    pub samples: Vec<HistorySample>,
}

impl HistoryData {
    /// The peak solar production across the series (0 if empty).
    #[must_use]
    pub fn peak_pv_watts(&self) -> f64 {
        self.samples
            .iter()
            .map(|s| s.pv_watts)
            .fold(0.0, f64::max)
    }
}

impl From<&HistorySeries> for HistoryData {
    fn from(s: &HistorySeries) -> Self {
        Self {
            period: s.period.clone(),
            samples: s
                .time_series
                .iter()
                .map(|p| HistorySample {
                    timestamp: p.timestamp.clone(),
                    pv_watts: p.solar_power,
                    battery_watts: p.battery_power,
                    grid_watts: p.grid_power,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet_api::types::{Envelope, HistorySeries};

    #[test]
    fn daterange_validates_order() {
        assert!(DateRange::new(100, 200).is_ok());
        assert!(DateRange::new(200, 100).is_err());
        let r = DateRange::new(100, 250).unwrap();
        assert_eq!(r.duration_secs(), 150);
    }

    #[test]
    fn last_hours_spans_back_from_now() {
        let now = 1_000_000;
        let r = DateRange::last_hours(now, 24);
        assert_eq!(r.end_unix, now);
        assert_eq!(r.start_unix, now - 24 * 3600);
    }

    #[test]
    fn last_hours_saturates_at_zero() {
        let r = DateRange::last_hours(10, 24);
        assert_eq!(r.start_unix, 0);
    }

    #[test]
    fn maps_from_history_series() {
        let json = r#"{"response":{"period":"day","time_series":[
            {"timestamp":"t0","solar_power":0,"battery_power":500,"grid_power":-500},
            {"timestamp":"t1","solar_power":4000,"battery_power":-1500,"grid_power":0}
        ]}}"#;
        let wire = serde_json::from_str::<Envelope<HistorySeries>>(json).unwrap().response;
        let h = HistoryData::from(&wire);
        assert_eq!(h.period, "day");
        assert_eq!(h.samples.len(), 2);
        assert_eq!(h.samples[1].timestamp, "t1");
        assert!((h.samples[1].pv_watts - 4000.0).abs() < f64::EPSILON);
        assert!((h.samples[0].grid_watts - -500.0).abs() < f64::EPSILON);
    }

    #[test]
    fn peak_pv_finds_the_max() {
        let json = r#"{"response":{"period":"day","time_series":[
            {"timestamp":"t0","solar_power":1000,"battery_power":0,"grid_power":0},
            {"timestamp":"t1","solar_power":4200,"battery_power":0,"grid_power":0}
        ]}}"#;
        let wire = serde_json::from_str::<Envelope<HistorySeries>>(json).unwrap().response;
        let h = HistoryData::from(&wire);
        assert!((h.peak_pv_watts() - 4200.0).abs() < f64::EPSILON);
    }

    #[test]
    fn peak_pv_of_empty_is_zero() {
        let h = HistoryData {
            period: "day".into(),
            samples: vec![],
        };
        assert!(h.peak_pv_watts().abs() < f64::EPSILON);
    }
}
