// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! A time range and the historical power series over it.

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
        let r = DateRange::last_hours(10_000, 24);
        assert_eq!(r.end_unix, 10_000);
        assert_eq!(r.start_unix, 10_000 - 24 * 3600);
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
