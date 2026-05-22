// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: evcc-io/evcc@7303a5b476be7fa3da35807df899651f47b3d2f0 core/loadpoint_session.go,
//         core/session/session.go.
//
//! Charge session — the energy / cost record kept for each completed
//! loadpoint plug-in/plug-out cycle.

use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime};

/// One completed (or in-progress) charge session.
///
/// Upstream `core/session/session.go::Session`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub loadpoint: String,
    pub vehicle: Option<String>,
    pub started: SystemTime,
    pub finished: Option<SystemTime>,
    pub energy_kwh: f64,
    pub solar_kwh: f64,
    pub price_eur: f64,
    pub co2_g: f64,
}

impl Session {
    /// Start a new session at `started`.
    #[must_use]
    pub fn start(loadpoint: impl Into<String>, vehicle: Option<String>, started: SystemTime) -> Self {
        Self {
            loadpoint: loadpoint.into(),
            vehicle,
            started,
            finished: None,
            energy_kwh: 0.0,
            solar_kwh: 0.0,
            price_eur: 0.0,
            co2_g: 0.0,
        }
    }

    /// Record one tick worth of charge. Source: upstream
    /// `core/loadpoint_session.go::sessionAddChargedEnergy`.
    pub fn tick(&mut self, power_kw: f64, solar_share: f64, dt: Duration, price_per_kwh: f64, co2_g_per_kwh: f64) {
        let hours = dt.as_secs_f64() / 3600.0;
        let energy_kwh = power_kw * hours;
        self.energy_kwh += energy_kwh;
        self.solar_kwh += energy_kwh * solar_share.clamp(0.0, 1.0);
        self.price_eur += energy_kwh * price_per_kwh;
        self.co2_g += energy_kwh * co2_g_per_kwh;
    }

    /// Mark the session finished.
    pub fn finish(&mut self, finished: SystemTime) {
        self.finished = Some(finished);
    }

    /// Returns `solar_kwh / energy_kwh` (autarky / solar share). `0.0`
    /// if the session has no energy.
    #[must_use]
    pub fn solar_share(&self) -> f64 {
        if self.energy_kwh <= 0.0 {
            0.0
        } else {
            (self.solar_kwh / self.energy_kwh).clamp(0.0, 1.0)
        }
    }

    /// Duration of the session if finished, else `None`.
    #[must_use]
    pub fn duration(&self) -> Option<Duration> {
        let finished = self.finished?;
        finished.duration_since(self.started).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_starts_empty() {
        let t0 = SystemTime::UNIX_EPOCH;
        let s = Session::start("lp1", Some("ev1".into()), t0);
        assert!(s.finished.is_none());
        assert!(s.energy_kwh.abs() < f64::EPSILON);
        assert!(s.solar_share().abs() < f64::EPSILON);
    }

    #[test]
    fn tick_accumulates_energy() {
        let t0 = SystemTime::UNIX_EPOCH;
        let mut s = Session::start("lp1", None, t0);
        s.tick(11.0, 1.0, Duration::from_secs(3600), 0.30, 200.0);
        assert!((s.energy_kwh - 11.0).abs() < 0.001);
        assert!((s.solar_kwh - 11.0).abs() < 0.001);
        assert!((s.price_eur - 3.30).abs() < 0.001);
        assert!((s.co2_g - 2200.0).abs() < 0.001);
    }

    #[test]
    fn solar_share_clamped() {
        let t0 = SystemTime::UNIX_EPOCH;
        let mut s = Session::start("lp1", None, t0);
        s.tick(11.0, 1.5, Duration::from_secs(3600), 0.0, 0.0);
        assert!((s.solar_share() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn finish_sets_finished_time_and_duration() {
        let t0 = SystemTime::UNIX_EPOCH;
        let mut s = Session::start("lp1", None, t0);
        s.finish(t0 + Duration::from_secs(3600));
        assert_eq!(s.duration(), Some(Duration::from_secs(3600)));
    }

    #[test]
    fn partial_solar_share_correct() {
        let t0 = SystemTime::UNIX_EPOCH;
        let mut s = Session::start("lp1", None, t0);
        s.tick(11.0, 0.5, Duration::from_secs(3600), 0.20, 100.0);
        assert!((s.solar_share() - 0.5).abs() < f64::EPSILON);
    }
}
