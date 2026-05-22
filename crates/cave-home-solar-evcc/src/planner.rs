// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: evcc-io/evcc@7303a5b476be7fa3da35807df899651f47b3d2f0 core/planner/planner.go,
//         core/loadpoint_plan.go, core/loadpoint_smartcost.go.
//
//! Charge planner — picks the cheapest set of tariff slots that can
//! deliver `target_kwh` energy by `deadline`, given the loadpoint's
//! `max_kw` ceiling.
//!
//! Upstream `core/planner/planner.go` is the canonical implementation;
//! cave-home reproduces the same selection algorithm: take cheapest
//! slots first, until the energy target is reached, sort selected slots
//! by start time, return the resulting plan.

use crate::error::{Error, Result};
use crate::tariff::{Tariff, TariffSample};
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// A single planned slot. Upstream type: `planner.Slot`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlanSlot {
    pub start: SystemTime,
    pub end: SystemTime,
    pub power_kw: f64,
    pub price_per_kwh: f64,
}

/// Plan = ordered slots + summary. Upstream return value of
/// `core/planner/planner.go::Plan`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub slots: Vec<PlanSlot>,
    pub total_kwh: f64,
    pub total_cost: f64,
}

/// Planner inputs.
#[derive(Debug, Clone, Copy)]
pub struct Planner {
    pub max_kw: f64,
    pub min_kw: f64,
}

impl Planner {
    #[must_use]
    pub const fn new(max_kw: f64, min_kw: f64) -> Self {
        Self { max_kw, min_kw }
    }

    /// Plan a charge session.
    ///
    /// * `target_kwh` — energy needed.
    /// * `deadline`   — must complete before this time.
    /// * `tariff`     — tariff time-series. Only slots that start
    ///   strictly before `deadline` are considered.
    ///
    /// Source: upstream `core/planner/planner.go::Plan` — "find cheapest
    /// continuous sequence of slots that delivers `requiredDuration`".
    pub fn plan(&self, target_kwh: f64, deadline: SystemTime, tariff: &Tariff) -> Result<Plan> {
        if target_kwh <= 0.0 {
            return Ok(Plan {
                slots: Vec::new(),
                total_kwh: 0.0,
                total_cost: 0.0,
            });
        }
        if self.max_kw <= 0.0 {
            return Err(Error::PlanInfeasible {
                target_kwh,
                horizon_h: 0,
                max_kw: self.max_kw,
            });
        }

        // Filter samples to those that finish at or before `deadline`.
        let in_horizon: Vec<TariffSample> = tariff
            .samples
            .iter()
            .copied()
            .filter(|s| s.end <= deadline)
            .collect();

        if in_horizon.is_empty() {
            return Err(Error::PlanInfeasible {
                target_kwh,
                horizon_h: 0,
                max_kw: self.max_kw,
            });
        }

        // Sort by price ascending.
        let mut ranked = in_horizon.clone();
        ranked.sort_by(|a, b| a.value.partial_cmp(&b.value).unwrap_or(std::cmp::Ordering::Equal));

        let mut selected = Vec::new();
        let mut energy_so_far = 0.0f64;
        let mut cost_so_far = 0.0f64;
        for sample in ranked {
            if energy_so_far >= target_kwh {
                break;
            }
            let slot_h = sample.duration().as_secs_f64() / 3600.0;
            // Cap power at the loadpoint ceiling.
            let slot_kwh = self.max_kw * slot_h;
            let needed = target_kwh - energy_so_far;
            let take = slot_kwh.min(needed);
            let kw = take / slot_h.max(f64::EPSILON);
            // Reject sub-min-current slivers — they cannot actually be
            // delivered by the hardware. Upstream: smart-cost planner
            // skips a slot when `kw < MinCurrent power`.
            if kw < self.min_kw && take < slot_kwh {
                continue;
            }
            selected.push(PlanSlot {
                start: sample.start,
                end: sample.end,
                power_kw: kw,
                price_per_kwh: sample.value,
            });
            energy_so_far += take;
            cost_so_far += take * sample.value;
        }

        if energy_so_far + 1e-9 < target_kwh {
            return Err(Error::PlanInfeasible {
                target_kwh,
                horizon_h: in_horizon.len() as u32,
                max_kw: self.max_kw,
            });
        }

        selected.sort_by_key(|s| s.start);
        Ok(Plan {
            slots: selected,
            total_kwh: energy_so_far,
            total_cost: cost_so_far,
        })
    }

    /// Returns `true` if the present time falls inside any plan slot.
    /// Source: upstream `core/loadpoint_plan.go::planActive`.
    #[must_use]
    pub fn is_active(plan: &Plan, now: SystemTime) -> bool {
        plan.slots.iter().any(|s| s.start <= now && now < s.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tariff::TariffKind;
    use std::time::Duration;

    fn slot(start: SystemTime, hours: u64, value: f64) -> TariffSample {
        TariffSample {
            start,
            end: start + Duration::from_secs(hours * 3600),
            value,
        }
    }

    #[test]
    fn zero_target_returns_empty_plan() {
        let p = Planner::new(11.0, 1.4);
        let t = Tariff::new("epex", TariffKind::PriceEurPerKwh);
        let plan = p
            .plan(0.0, SystemTime::UNIX_EPOCH + Duration::from_secs(7200), &t)
            .unwrap();
        assert!(plan.slots.is_empty());
    }

    #[test]
    fn empty_tariff_in_horizon_is_infeasible() {
        let p = Planner::new(11.0, 1.4);
        let t = Tariff::new("epex", TariffKind::PriceEurPerKwh);
        let r = p.plan(5.0, SystemTime::UNIX_EPOCH, &t);
        assert!(matches!(r, Err(Error::PlanInfeasible { .. })));
    }

    #[test]
    fn picks_cheapest_slot_first() {
        let t0 = SystemTime::UNIX_EPOCH;
        let mut t = Tariff::new("epex", TariffKind::PriceEurPerKwh);
        t.extend([
            slot(t0, 1, 0.30),
            slot(t0 + Duration::from_secs(3600), 1, 0.10),
            slot(t0 + Duration::from_secs(7200), 1, 0.20),
        ]);
        let p = Planner::new(11.0, 1.4);
        let plan = p.plan(11.0, t0 + Duration::from_secs(10800), &t).unwrap();
        assert_eq!(plan.slots.len(), 1);
        assert!((plan.slots[0].price_per_kwh - 0.10).abs() < f64::EPSILON);
    }

    #[test]
    fn plan_orders_selected_slots_by_start() {
        let t0 = SystemTime::UNIX_EPOCH;
        let mut t = Tariff::new("epex", TariffKind::PriceEurPerKwh);
        t.extend([
            slot(t0, 1, 0.30),
            slot(t0 + Duration::from_secs(3600), 1, 0.10),
            slot(t0 + Duration::from_secs(7200), 1, 0.20),
        ]);
        let p = Planner::new(11.0, 1.4);
        // 22 kWh @ 11 kW → 2 slots → cheapest are 0.10 and 0.20.
        let plan = p.plan(22.0, t0 + Duration::from_secs(10800), &t).unwrap();
        assert_eq!(plan.slots.len(), 2);
        assert!(plan.slots[0].start < plan.slots[1].start);
    }

    #[test]
    fn target_exceeds_horizon_is_infeasible() {
        let t0 = SystemTime::UNIX_EPOCH;
        let mut t = Tariff::new("epex", TariffKind::PriceEurPerKwh);
        t.extend([slot(t0, 1, 0.20)]);
        let p = Planner::new(11.0, 1.4);
        let r = p.plan(100.0, t0 + Duration::from_secs(3600), &t);
        assert!(matches!(r, Err(Error::PlanInfeasible { .. })));
    }

    #[test]
    fn slot_outside_deadline_is_excluded() {
        let t0 = SystemTime::UNIX_EPOCH;
        let mut t = Tariff::new("epex", TariffKind::PriceEurPerKwh);
        t.extend([
            slot(t0, 1, 0.30),
            slot(t0 + Duration::from_secs(3600), 1, 0.10),
            slot(t0 + Duration::from_secs(7200), 1, 0.05),
        ]);
        let p = Planner::new(11.0, 1.4);
        // Deadline at t0 + 2 h excludes the 0.05 slot.
        let plan = p.plan(11.0, t0 + Duration::from_secs(7200), &t).unwrap();
        assert!((plan.slots[0].price_per_kwh - 0.10).abs() < f64::EPSILON);
    }

    #[test]
    fn plan_total_cost_correct() {
        let t0 = SystemTime::UNIX_EPOCH;
        let mut t = Tariff::new("epex", TariffKind::PriceEurPerKwh);
        t.extend([
            slot(t0, 1, 0.30),
            slot(t0 + Duration::from_secs(3600), 1, 0.10),
        ]);
        let p = Planner::new(11.0, 1.4);
        let plan = p.plan(11.0, t0 + Duration::from_secs(7200), &t).unwrap();
        assert!((plan.total_cost - 1.1).abs() < 0.001);
    }

    #[test]
    fn is_active_truthy_inside_slot() {
        let t0 = SystemTime::UNIX_EPOCH;
        let plan = Plan {
            slots: vec![PlanSlot {
                start: t0,
                end: t0 + Duration::from_secs(3600),
                power_kw: 11.0,
                price_per_kwh: 0.10,
            }],
            total_kwh: 11.0,
            total_cost: 1.1,
        };
        assert!(Planner::is_active(&plan, t0 + Duration::from_secs(1800)));
        assert!(!Planner::is_active(&plan, t0 + Duration::from_secs(3601)));
    }

    #[test]
    fn negative_max_kw_infeasible() {
        let p = Planner::new(-1.0, 0.0);
        let t = Tariff::new("epex", TariffKind::PriceEurPerKwh);
        let r = p.plan(5.0, SystemTime::UNIX_EPOCH, &t);
        assert!(matches!(r, Err(Error::PlanInfeasible { .. })));
    }
}
