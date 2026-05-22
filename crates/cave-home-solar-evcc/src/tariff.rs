// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: evcc-io/evcc@7303a5b476be7fa3da35807df899651f47b3d2f0 tariff/, api/tariff.go.
//
//! Tariff samples — price-per-kWh or CO₂-per-kWh series consumed by
//! the planner to pick cheap/clean charge windows.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime};

/// What the tariff series measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TariffKind {
    /// €/kWh (or local currency) — upstream `api.TariffPrice`.
    PriceEurPerKwh,
    /// g CO₂ / kWh — upstream `api.TariffCo2`.
    Co2GPerKwh,
}

/// A single forecast/tariff sample. Upstream type: `api.Rate`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TariffSample {
    pub start: SystemTime,
    pub end: SystemTime,
    pub value: f64,
}

impl TariffSample {
    /// Duration of this slot.
    #[must_use]
    pub fn duration(&self) -> Duration {
        self.end.duration_since(self.start).unwrap_or(Duration::ZERO)
    }
}

/// A tariff time-series. Owns its samples and exposes lookup +
/// cheapest-window selection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tariff {
    pub name: &'static str,
    pub kind: TariffKind,
    pub samples: Vec<TariffSample>,
}

impl Tariff {
    #[must_use]
    pub fn new(name: &'static str, kind: TariffKind) -> Self {
        Self {
            name,
            kind,
            samples: Vec::new(),
        }
    }

    pub fn extend(&mut self, samples: impl IntoIterator<Item = TariffSample>) {
        self.samples.extend(samples);
        self.samples.sort_by_key(|s| s.start);
    }

    /// Returns the cheapest slot. Source: upstream
    /// `tariff/util.go::Sort` selects ascending by `Rate.Value`.
    pub fn cheapest_slot(&self) -> Result<TariffSample> {
        self.samples
            .iter()
            .copied()
            .min_by(|a, b| a.value.partial_cmp(&b.value).unwrap_or(std::cmp::Ordering::Equal))
            .ok_or(Error::TariffEmpty(self.name))
    }

    /// Returns the cheapest contiguous window of total energy `kwh` at
    /// `kw` power. Source: upstream `core/planner/planner.go::Plan`.
    pub fn cheapest_window(&self, kwh: f64, kw: f64) -> Result<Vec<TariffSample>> {
        if self.samples.is_empty() {
            return Err(Error::TariffEmpty(self.name));
        }
        // Convert energy target into slot count assuming `kw` is constant
        // and slots are uniform 60-minute windows by convention. For
        // partial-slot accuracy, callers can pre-compute.
        let energy_per_slot = kw * (60.0 / 60.0); // 1 h slots
        let slot_count = (kwh / energy_per_slot).ceil() as usize;
        if slot_count == 0 {
            return Ok(Vec::new());
        }
        if slot_count > self.samples.len() {
            return Err(Error::PlanInfeasible {
                target_kwh: kwh,
                horizon_h: self.samples.len() as u32,
                max_kw: kw,
            });
        }
        let mut sorted = self.samples.clone();
        sorted.sort_by(|a, b| a.value.partial_cmp(&b.value).unwrap_or(std::cmp::Ordering::Equal));
        sorted.truncate(slot_count);
        sorted.sort_by_key(|s| s.start);
        Ok(sorted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn slot(start: SystemTime, hours: u64, value: f64) -> TariffSample {
        TariffSample {
            start,
            end: start + Duration::from_secs(hours * 3600),
            value,
        }
    }

    #[test]
    fn cheapest_slot_on_empty_errs() {
        let t = Tariff::new("epex", TariffKind::PriceEurPerKwh);
        assert!(matches!(t.cheapest_slot(), Err(Error::TariffEmpty("epex"))));
    }

    #[test]
    fn cheapest_slot_picks_min() {
        let t0 = SystemTime::UNIX_EPOCH;
        let mut t = Tariff::new("epex", TariffKind::PriceEurPerKwh);
        t.extend([
            slot(t0, 1, 0.20),
            slot(t0 + Duration::from_secs(3600), 1, 0.10),
            slot(t0 + Duration::from_secs(7200), 1, 0.30),
        ]);
        assert!((t.cheapest_slot().unwrap().value - 0.10).abs() < f64::EPSILON);
    }

    #[test]
    fn cheapest_window_target_energy_picks_cheapest_slots() {
        let t0 = SystemTime::UNIX_EPOCH;
        let mut t = Tariff::new("epex", TariffKind::PriceEurPerKwh);
        t.extend([
            slot(t0, 1, 0.20),
            slot(t0 + Duration::from_secs(3600), 1, 0.10),
            slot(t0 + Duration::from_secs(7200), 1, 0.30),
            slot(t0 + Duration::from_secs(10800), 1, 0.15),
        ]);
        // 11 kWh @ 11 kW = 1 slot; cheapest is 0.10.
        let w = t.cheapest_window(11.0, 11.0).unwrap();
        assert_eq!(w.len(), 1);
        assert!((w[0].value - 0.10).abs() < f64::EPSILON);
    }

    #[test]
    fn cheapest_window_multiple_slots_sorted_by_time() {
        let t0 = SystemTime::UNIX_EPOCH;
        let mut t = Tariff::new("epex", TariffKind::PriceEurPerKwh);
        t.extend([
            slot(t0, 1, 0.20),
            slot(t0 + Duration::from_secs(3600), 1, 0.10),
            slot(t0 + Duration::from_secs(7200), 1, 0.30),
            slot(t0 + Duration::from_secs(10800), 1, 0.15),
        ]);
        // 22 kWh @ 11 kW = 2 slots; should be 0.10 and 0.15, time-sorted.
        let w = t.cheapest_window(22.0, 11.0).unwrap();
        assert_eq!(w.len(), 2);
        assert!((w[0].value - 0.10).abs() < f64::EPSILON);
        assert!((w[1].value - 0.15).abs() < f64::EPSILON);
    }

    #[test]
    fn cheapest_window_infeasible() {
        let t0 = SystemTime::UNIX_EPOCH;
        let mut t = Tariff::new("epex", TariffKind::PriceEurPerKwh);
        t.extend([slot(t0, 1, 0.20)]);
        assert!(matches!(
            t.cheapest_window(100.0, 11.0),
            Err(Error::PlanInfeasible { .. })
        ));
    }

    #[test]
    fn sample_duration_correct() {
        let t0 = SystemTime::UNIX_EPOCH;
        let s = slot(t0, 2, 0.0);
        assert_eq!(s.duration(), Duration::from_secs(7200));
    }
}
