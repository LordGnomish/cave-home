// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: evcc-io/evcc@7303a5b476be7fa3da35807df899651f47b3d2f0 core/loadpoint.go (err vocabulary)
//! EVCC core error type — narrow enum, no `anyhow`. Charter §6 strict-TDD demands
//! callers can match exhaustively.

use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum Error {
    /// A meter reading was missing or its session-key dependency hadn't been
    /// resolved yet. Upstream: `core/site.go::SitePower` early-returns.
    #[error("meter `{0}` has not produced a reading yet")]
    MeterUnavailable(&'static str),

    /// Loadpoint was asked to charge while disabled, or above its configured
    /// `MaxCurrent`. Upstream: `core/loadpoint.go::setLimit`.
    #[error("loadpoint `{name}` rejected current {requested_a}A (range {min_a}..{max_a}A)")]
    CurrentOutOfRange {
        name: String,
        requested_a: u16,
        min_a: u16,
        max_a: u16,
    },

    /// Phase switching requested for a 1-phase-only charger. Upstream:
    /// `core/loadpoint_phases.go::pvScalePhases`.
    #[error("loadpoint `{0}` does not support phase switching")]
    PhaseSwitchUnsupported(String),

    /// Planner asked to schedule above its horizon. Upstream:
    /// `core/planner/planner.go`.
    #[error("plan target {target_kwh} kWh exceeds horizon ({horizon_h}h × max {max_kw} kW)")]
    PlanInfeasible {
        target_kwh: f64,
        horizon_h: u32,
        max_kw: f64,
    },

    /// Battery / vehicle SoC reading outside `0..=100`.
    #[error("invalid SoC value {0}% (expected 0..=100)")]
    InvalidSoc(i16),

    /// A configured tariff sample carried no provider data.
    #[error("tariff `{0}` has no samples for the requested window")]
    TariffEmpty(&'static str),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_unavailable_message_is_actionable() {
        let e = Error::MeterUnavailable("grid");
        assert_eq!(e.to_string(), "meter `grid` has not produced a reading yet");
    }

    #[test]
    fn current_range_error_carries_all_fields() {
        let e = Error::CurrentOutOfRange {
            name: "lp1".into(),
            requested_a: 32,
            min_a: 6,
            max_a: 16,
        };
        assert!(e.to_string().contains("32A"));
        assert!(e.to_string().contains("6..16A"));
    }

    #[test]
    fn invalid_soc_carries_value() {
        assert_eq!(Error::InvalidSoc(120).to_string(), "invalid SoC value 120% (expected 0..=100)");
    }
}
