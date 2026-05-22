// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: evcc-io/evcc@7303a5b476be7fa3da35807df899651f47b3d2f0 core/site.go::loopOnce,
//         cmd/loop.go.
//
//! Surplus-loop tick — the function that runs every ~10 seconds in
//! the upstream `evcc` daemon. It ties the meters, prioritizer,
//! planner, and loadpoint decision steps together.

use crate::error::Result;
use crate::loadpoint::Loadpoint;
use crate::prioritizer::{Allocation, Prioritizer};
use crate::site::{Site, Surplus};
use serde::{Deserialize, Serialize};

/// Decision output of a single surplus-loop tick.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickOutcome {
    pub surplus: Surplus,
    /// One decision per loadpoint, in registration order.
    pub decisions: Vec<LoadpointDecision>,
}

/// What the surplus loop decided for a single loadpoint this tick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadpointDecision {
    pub name: String,
    pub allotted_w: u32,
    pub charge_current_a: u16,
}

/// Run one tick of the surplus loop. Upstream `core/site.go::loopOnce`.
pub fn run_tick(site: &Site, prioritizer: &Prioritizer) -> Result<TickOutcome> {
    let surplus = site.compute_surplus()?;
    let allocations = prioritizer.distribute(surplus.available_w, &site.loadpoints);

    let decisions: Vec<LoadpointDecision> = site
        .loadpoints
        .iter()
        .enumerate()
        .map(|(idx, lp)| {
            let allotted_w = allocations
                .iter()
                .find(|a| a.id == idx)
                .map(|a| a.allotted_w)
                .unwrap_or(0);
            let next_a = lp.next_decision(allotted_w as i32);
            LoadpointDecision {
                name: lp.name.clone(),
                allotted_w,
                charge_current_a: next_a,
            }
        })
        .collect();

    Ok(TickOutcome { surplus, decisions })
}

#[allow(dead_code)]
fn _readable_for_review(_alloc: &[Allocation], _lps: &[Loadpoint]) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loadpoint::{ChargeMode, Loadpoint};
    use crate::prioritizer::Priority;
    use crate::site::{BatteryMeter, GridMeter, PvMeter};

    #[test]
    fn tick_with_no_loadpoints_returns_empty_decisions() {
        let mut s = Site::new("home");
        s.set_grid(GridMeter { power_w: -5_000 });
        s.set_pv(PvMeter { power_w: 6_000 });
        let p = Prioritizer::new();
        let out = run_tick(&s, &p).unwrap();
        assert!(out.decisions.is_empty());
        assert!(out.surplus.available_w > 0);
    }

    #[test]
    fn tick_routes_surplus_to_priority_loadpoint() {
        let mut s = Site::new("home");
        s.set_grid(GridMeter { power_w: -12_000 });
        s.set_pv(PvMeter { power_w: 12_000 });
        s.set_battery(BatteryMeter {
            power_w: 0,
            soc_percent: 80,
        })
        .unwrap();
        let mut lp1 = Loadpoint::new_ev("lp1");
        lp1.set_mode(ChargeMode::Pv);
        let mut lp2 = Loadpoint::new_ev("lp2");
        lp2.set_mode(ChargeMode::Pv);
        s.add_loadpoint(lp1);
        s.add_loadpoint(lp2);
        let mut p = Prioritizer::new();
        p.upsert(Priority {
            id: 0,
            score: 5,
            since_tick: 0,
        });
        p.upsert(Priority {
            id: 1,
            score: 10,
            since_tick: 0,
        });
        let out = run_tick(&s, &p).unwrap();
        // lp2 has higher score → should get the bigger share.
        let lp1_dec = out.decisions.iter().find(|d| d.name == "lp1").unwrap();
        let lp2_dec = out.decisions.iter().find(|d| d.name == "lp2").unwrap();
        assert!(lp2_dec.charge_current_a >= lp1_dec.charge_current_a);
    }

    #[test]
    fn tick_decision_zero_when_no_surplus() {
        let mut s = Site::new("home");
        s.set_grid(GridMeter { power_w: 5_000 });
        s.set_pv(PvMeter { power_w: 100 });
        let mut lp1 = Loadpoint::new_ev("lp1");
        lp1.set_mode(ChargeMode::Pv);
        s.add_loadpoint(lp1);
        let mut p = Prioritizer::new();
        p.upsert(Priority {
            id: 0,
            score: 1,
            since_tick: 0,
        });
        let out = run_tick(&s, &p).unwrap();
        assert_eq!(out.decisions[0].charge_current_a, 0);
    }

    #[test]
    fn tick_propagates_meter_unavailable() {
        let s = Site::new("home");
        let p = Prioritizer::new();
        assert!(run_tick(&s, &p).is_err());
    }
}
