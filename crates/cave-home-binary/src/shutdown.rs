// SPDX-License-Identifier: Apache-2.0
//! Graceful-shutdown planning.
//!
//! Shutdown is the bring-up plan reversed: the household-facing surfaces stop
//! first (so nothing new arrives), then the pillars, then the core, the broker,
//! and finally the orchestration layer that hosts them all. This is **pure
//! planning** — the actual stop calls + signal handling are deferred Phase 1b.

use crate::bootstrap::Plan;
use crate::Component;

/// An ordered graceful-shutdown plan (the bring-up order, reversed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownPlan {
    steps: Vec<Component>,
}

impl ShutdownPlan {
    /// Derive the shutdown plan from a bring-up [`Plan`] by reversing it.
    #[must_use]
    pub fn from_bootstrap(plan: &Plan) -> Self {
        let mut steps = plan.steps().to_vec();
        steps.reverse();
        Self { steps }
    }

    /// The shutdown steps, in stop order.
    #[must_use]
    pub fn steps(&self) -> &[Component] {
        &self.steps
    }

    /// Number of components to stop.
    #[must_use]
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Whether there is nothing to stop.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, ConfigLayer};

    fn hub_plan() -> Plan {
        let cfg = Config::from_layers(&[ConfigLayer::defaults()]).expect("valid");
        Plan::compute(&cfg).expect("plan")
    }

    #[test]
    fn shutdown_is_the_exact_reverse_of_bootstrap() {
        let boot = hub_plan();
        let down = ShutdownPlan::from_bootstrap(&boot);
        let mut expected = boot.steps().to_vec();
        expected.reverse();
        assert_eq!(down.steps(), expected.as_slice());
        assert_eq!(down.len(), boot.len());
    }

    #[test]
    fn portal_stops_first_and_orchestration_last() {
        let down = ShutdownPlan::from_bootstrap(&hub_plan());
        assert_eq!(down.steps().first(), Some(&Component::Portal));
        assert_eq!(down.steps().last(), Some(&Component::Orchestration));
    }

    #[test]
    fn double_reverse_returns_to_bootstrap_order() {
        let boot = hub_plan();
        let down = ShutdownPlan::from_bootstrap(&boot);
        // Reversing the shutdown plan reconstructs the bring-up order.
        let mut roundtrip = down.steps().to_vec();
        roundtrip.reverse();
        assert_eq!(roundtrip.as_slice(), boot.steps());
    }
}
