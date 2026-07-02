// SPDX-License-Identifier: Apache-2.0
//! Startup bring-up planning.
//!
//! Given a validated [`Config`](crate::Config), compute the ordered list of
//! [`Component`]s to bring up, respecting their dependencies. This is **pure
//! planning** — nothing is launched, no async runtime is touched, no I/O
//! happens. The plan is what [`server::run`](crate::server) executes.
//!
//! Ordering rules (Charter §5):
//! - The orchestration layer hosts everything, so it starts first.
//! - The broker carries device messaging, so it precedes the pillars that
//!   publish/subscribe through it.
//! - The automation core precedes the surfaces that consume its state.
//! - The Portal (the household-facing surface) comes up last so it only ever
//!   shows the household a home that is already running.
//!
//! All components in a plan run **in the one binary** (Charter §5). There is no
//! per-component OS process; [`Plan::is_single_binary`] makes that invariant
//! checkable.

use crate::config::Config;
use crate::Component;
use std::fmt;

/// The bring-up order, lowest rank first. Two components with the same role get
/// a stable tiebreak by [`Component::ALL`] order. This is the single source of
/// truth for both startup and (reversed) shutdown ordering.
const fn start_rank(c: Component) -> u8 {
    match c {
        Component::Orchestration => 0,
        Component::Broker => 1,
        Component::Core => 2,
        // Integrations and camera ingest both sit on the core; same tier.
        Component::Integrations | Component::Cameras => 3,
        Component::Voice => 4,
        Component::Portal => 5,
    }
}

/// An ordered, validated bring-up plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    steps: Vec<Component>,
}

/// Why a plan could not be computed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// The config enabled no components (should be caught at config validation,
    /// repeated here so the planner is safe in isolation).
    Empty,
    /// A pillar was requested without the orchestration layer that hosts it.
    MissingOrchestration,
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "there are no home features to start"),
            Self::MissingOrchestration => {
                write!(f, "the home foundation must start before the other features")
            }
        }
    }
}

impl std::error::Error for PlanError {}

impl Plan {
    /// Compute the ordered bring-up plan for a validated config.
    ///
    /// # Errors
    /// Returns a [`PlanError`] if the component set is empty or omits the
    /// orchestration layer while requesting other pillars.
    pub fn compute(config: &Config) -> Result<Self, PlanError> {
        if config.components.is_empty() {
            return Err(PlanError::Empty);
        }
        let has_other = config
            .components
            .iter()
            .any(|c| *c != Component::Orchestration);
        if has_other && !config.components.contains(&Component::Orchestration) {
            return Err(PlanError::MissingOrchestration);
        }

        let mut steps: Vec<Component> = config.components.iter().copied().collect();
        // Stable sort by start rank; ties keep canonical Component order, which
        // BTreeSet iteration already gives us.
        steps.sort_by_key(|c| start_rank(*c));
        Ok(Self { steps })
    }

    /// The bring-up steps, in start order.
    #[must_use]
    pub fn steps(&self) -> &[Component] {
        &self.steps
    }

    /// The number of components this plan brings up.
    #[must_use]
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Whether the plan is empty (it never is for a validated config, but the
    /// accessor keeps Clippy and callers honest).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Whether the plan respects the single-binary invariant: every component
    /// runs in the same process, the orchestration layer leads, and the Portal
    /// (if present) is last. Charter §5.
    #[must_use]
    pub fn is_single_binary(&self) -> bool {
        if self.steps.is_empty() {
            return false;
        }
        // Orchestration, if present, must be the very first step.
        if self.steps.contains(&Component::Orchestration)
            && self.steps.first() != Some(&Component::Orchestration)
        {
            return false;
        }
        // Portal, if present, must be the very last step.
        if self.steps.contains(&Component::Portal)
            && self.steps.last() != Some(&Component::Portal)
        {
            return false;
        }
        // No component appears twice — one in-process instance each.
        let mut seen = self.steps.clone();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        seen.len() == before
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, ConfigLayer, Layer, NodeRole};
    use std::collections::BTreeSet;

    fn config_with(components: BTreeSet<Component>) -> Config {
        let mut flags = ConfigLayer::empty(Layer::Flags);
        flags.role = Some(NodeRole::Hub);
        flags.components = Some(components);
        Config::resolve_standard(
            &ConfigLayer::empty(Layer::File),
            &ConfigLayer::empty(Layer::Env),
            &flags,
        )
        .expect("config valid")
    }

    fn hub_config() -> Config {
        Config::from_layers(&[ConfigLayer::defaults()]).expect("defaults valid")
    }

    #[test]
    fn hub_plan_starts_orchestration_first_and_portal_last() {
        let plan = Plan::compute(&hub_config()).expect("plan");
        assert_eq!(plan.steps().first(), Some(&Component::Orchestration));
        assert_eq!(plan.steps().last(), Some(&Component::Portal));
    }

    #[test]
    fn broker_precedes_core_which_precedes_pillars() {
        let plan = Plan::compute(&hub_config()).expect("plan");
        let steps = plan.steps();
        let pos = |c: Component| steps.iter().position(|x| *x == c).expect("present");
        assert!(pos(Component::Orchestration) < pos(Component::Broker));
        assert!(pos(Component::Broker) < pos(Component::Core));
        assert!(pos(Component::Core) < pos(Component::Integrations));
        assert!(pos(Component::Core) < pos(Component::Portal));
    }

    #[test]
    fn full_hub_plan_has_every_component_once() {
        let plan = Plan::compute(&hub_config()).expect("plan");
        assert_eq!(plan.len(), Component::ALL.len());
        for c in Component::ALL {
            assert_eq!(
                plan.steps().iter().filter(|x| **x == c).count(),
                1,
                "{c:?} should appear exactly once"
            );
        }
    }

    #[test]
    fn single_binary_invariant_holds_for_hub() {
        let plan = Plan::compute(&hub_config()).expect("plan");
        assert!(plan.is_single_binary());
    }

    #[test]
    fn minimal_plan_orchestration_only_is_single_binary() {
        let cfg = config_with([Component::Orchestration].into_iter().collect());
        let plan = Plan::compute(&cfg).expect("plan");
        assert_eq!(plan.steps(), &[Component::Orchestration]);
        assert!(plan.is_single_binary());
    }

    #[test]
    fn subset_plan_keeps_relative_order() {
        let cfg = config_with(
            [Component::Orchestration, Component::Core, Component::Portal]
                .into_iter()
                .collect(),
        );
        let plan = Plan::compute(&cfg).expect("plan");
        assert_eq!(
            plan.steps(),
            &[Component::Orchestration, Component::Core, Component::Portal]
        );
        assert!(plan.is_single_binary());
    }

    #[test]
    fn empty_component_plan_errors() {
        // Build a Plan directly from a hand-made empty config without going
        // through validation, to exercise the planner's own guard.
        let cfg = Config {
            node_name: "x".to_string(),
            role: NodeRole::Hub,
            components: BTreeSet::new(),
            data_dir: "/x".to_string(),
            bind_addr: "0.0.0.0".to_string(),
            bind_port: 1,
            log_level: crate::config::LogLevel::Info,
        };
        assert_eq!(Plan::compute(&cfg).unwrap_err(), PlanError::Empty);
    }

    #[test]
    fn pillar_without_orchestration_plan_errors() {
        let cfg = Config {
            node_name: "x".to_string(),
            role: NodeRole::Hub,
            components: [Component::Core].into_iter().collect(),
            data_dir: "/x".to_string(),
            bind_addr: "0.0.0.0".to_string(),
            bind_port: 1,
            log_level: crate::config::LogLevel::Info,
        };
        assert_eq!(
            Plan::compute(&cfg).unwrap_err(),
            PlanError::MissingOrchestration
        );
    }

    #[test]
    fn ml_node_plan_brings_up_cameras() {
        let mut flags = ConfigLayer::empty(Layer::Flags);
        flags.role = Some(NodeRole::Ml);
        let cfg = Config::resolve_standard(
            &ConfigLayer::empty(Layer::File),
            &ConfigLayer::empty(Layer::Env),
            &flags,
        )
        .expect("ml valid");
        let plan = Plan::compute(&cfg).expect("plan");
        assert_eq!(plan.steps().first(), Some(&Component::Orchestration));
        assert!(plan.steps().contains(&Component::Cameras));
        assert!(plan.is_single_binary());
    }
}
