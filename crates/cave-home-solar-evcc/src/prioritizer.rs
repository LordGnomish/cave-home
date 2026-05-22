// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
// Source: evcc-io/evcc@7303a5b476be7fa3da35807df899651f47b3d2f0 core/prioritizer/prioritizer.go.
//
//! Loadpoint prioritizer — when total surplus is insufficient to feed
//! every loadpoint, decide who gets it first.
//!
//! Upstream policy (`core/prioritizer/prioritizer.go`):
//!
//!   * Higher `Priority` wins.
//!   * Within equal priority, the loadpoint that has been waiting
//!     longest wins ("fair queueing").

use crate::loadpoint::Loadpoint;
use serde::{Deserialize, Serialize};

/// Lookup tag identifying a loadpoint inside a [`Prioritizer`].
pub type LoadpointId = usize;

/// Priority record. Upstream: `core.priorities[lp] = priority`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Priority {
    pub id: LoadpointId,
    /// Priority score — larger wins. Upstream uses `int`.
    pub score: i32,
    /// Monotonic tick counter — used to tie-break across equal scores.
    /// The loadpoint with the smaller `since_tick` (older waiter) wins.
    pub since_tick: u64,
}

/// Surplus distribution result. One entry per loadpoint id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allocation {
    pub id: LoadpointId,
    pub allotted_w: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Prioritizer {
    pub priorities: Vec<Priority>,
}

impl Prioritizer {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            priorities: Vec::new(),
        }
    }

    /// Register or update a loadpoint priority.
    pub fn upsert(&mut self, p: Priority) {
        if let Some(slot) = self.priorities.iter_mut().find(|x| x.id == p.id) {
            *slot = p;
        } else {
            self.priorities.push(p);
        }
    }

    /// Distribute `surplus_w` to the configured loadpoints in priority
    /// order. Returns one [`Allocation`] per loadpoint, even if its
    /// share is zero.
    ///
    /// `loadpoints` must be in registration order. Source: upstream
    /// `core/prioritizer/prioritizer.go::Demand`.
    #[must_use]
    pub fn distribute(&self, surplus_w: u32, loadpoints: &[Loadpoint]) -> Vec<Allocation> {
        let mut order: Vec<&Priority> = self.priorities.iter().collect();
        // Higher score first; older waiter first within ties.
        order.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.since_tick.cmp(&b.since_tick))
        });

        let mut remaining = surplus_w;
        let mut out: Vec<Allocation> = loadpoints
            .iter()
            .enumerate()
            .map(|(id, _)| Allocation {
                id,
                allotted_w: 0,
            })
            .collect();

        for p in order {
            let Some(lp) = loadpoints.get(p.id) else {
                continue;
            };
            // Upstream cap: phase × max × 230 V (single loadpoint cap).
            let cap = crate::loadpoint::Loadpoint::current_to_watts(
                lp.current_envelope.max_a,
                lp.phases,
            );
            let take = remaining.min(cap);
            if let Some(alloc) = out.iter_mut().find(|a| a.id == p.id) {
                alloc.allotted_w = take;
            }
            remaining = remaining.saturating_sub(take);
            if remaining == 0 {
                break;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loadpoint::{Loadpoint, PhaseCount};

    #[test]
    fn higher_priority_eats_first() {
        let lp1 = Loadpoint::new_ev("lp1");
        let lp2 = Loadpoint::new_ev("lp2");
        let mut p = Prioritizer::new();
        p.upsert(Priority {
            id: 0,
            score: 1,
            since_tick: 0,
        });
        p.upsert(Priority {
            id: 1,
            score: 10,
            since_tick: 0,
        });
        let alloc = p.distribute(11_040, &[lp1, lp2]);
        // 11.04 kW exactly fills one three-phase 16 A loadpoint; lp2 (id=1, score 10) wins.
        assert_eq!(alloc[1].allotted_w, 11_040);
        assert_eq!(alloc[0].allotted_w, 0);
    }

    #[test]
    fn older_waiter_wins_tie() {
        let lp1 = Loadpoint::new_ev("lp1");
        let lp2 = Loadpoint::new_ev("lp2");
        let mut p = Prioritizer::new();
        p.upsert(Priority {
            id: 0,
            score: 5,
            since_tick: 100,
        });
        p.upsert(Priority {
            id: 1,
            score: 5,
            since_tick: 5,
        }); // older
        let alloc = p.distribute(11_040, &[lp1, lp2]);
        assert_eq!(alloc[1].allotted_w, 11_040);
    }

    #[test]
    fn upsert_replaces_existing() {
        let mut p = Prioritizer::new();
        p.upsert(Priority {
            id: 0,
            score: 1,
            since_tick: 0,
        });
        p.upsert(Priority {
            id: 0,
            score: 9,
            since_tick: 0,
        });
        assert_eq!(p.priorities.len(), 1);
        assert_eq!(p.priorities[0].score, 9);
    }

    #[test]
    fn split_surplus_drains_to_zero() {
        let lp1 = Loadpoint::new_ev("lp1");
        let lp2 = Loadpoint::new_ev("lp2");
        let mut p = Prioritizer::new();
        p.upsert(Priority {
            id: 0,
            score: 1,
            since_tick: 0,
        });
        p.upsert(Priority {
            id: 1,
            score: 2,
            since_tick: 0,
        });
        // 15 kW > 11.04 kW cap; first goes max, leftover to second.
        let alloc = p.distribute(15_000, &[lp1, lp2]);
        let total: u32 = alloc.iter().map(|a| a.allotted_w).sum();
        assert_eq!(total, 15_000);
        assert_eq!(alloc[1].allotted_w, 11_040);
        assert_eq!(alloc[0].allotted_w, 15_000 - 11_040);
    }

    #[test]
    fn single_phase_charger_caps_correctly() {
        let mut lp1 = Loadpoint::new_ev("lp1");
        lp1.set_phases(PhaseCount::Single).unwrap();
        let mut p = Prioritizer::new();
        p.upsert(Priority {
            id: 0,
            score: 1,
            since_tick: 0,
        });
        let alloc = p.distribute(11_040, &[lp1]);
        // Cap is 230 × 16 = 3680 W (single phase).
        assert_eq!(alloc[0].allotted_w, 3_680);
    }
}
