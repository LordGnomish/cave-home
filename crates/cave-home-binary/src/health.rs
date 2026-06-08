// SPDX-License-Identifier: Apache-2.0
//! Health / readiness aggregation.
//!
//! Each in-process [`Component`] reports a [`HealthState`]. The binary's overall
//! verdict is the aggregation of those: it is **Up** only when every component
//! is up, **Down** when nothing is up, and **Degraded** in between. The verdict
//! drives the readiness answer the load-path and the household status view use.
//!
//! Pure logic: states are supplied, never probed here.

use crate::Component;
use std::collections::BTreeMap;
use std::fmt;

/// The state of a single component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthState {
    /// Fully running and serving.
    Up,
    /// Running but impaired (reduced function, retrying, partial).
    Degraded,
    /// Not running / failed.
    Down,
}

/// One component's reported health.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComponentHealth {
    pub component: Component,
    pub state: HealthState,
}

impl ComponentHealth {
    #[must_use]
    pub const fn new(component: Component, state: HealthState) -> Self {
        Self { component, state }
    }
}

/// The aggregated readiness of the whole binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Health {
    state: HealthState,
    components: BTreeMap<Component, HealthState>,
}

impl Health {
    /// Aggregate per-component states into an overall verdict.
    ///
    /// Rules:
    /// - No reports at all → [`HealthState::Down`] (nothing is running).
    /// - Every component up → [`HealthState::Up`].
    /// - Every component down → [`HealthState::Down`].
    /// - Anything in between (some up, some degraded, a partial outage) →
    ///   [`HealthState::Degraded`].
    ///
    /// Later reports for the same component replace earlier ones.
    #[must_use]
    pub fn aggregate(reports: &[ComponentHealth]) -> Self {
        let mut components: BTreeMap<Component, HealthState> = BTreeMap::new();
        for r in reports {
            components.insert(r.component, r.state);
        }
        let state = if components.is_empty() {
            HealthState::Down
        } else {
            let total = components.len();
            let up = components.values().filter(|s| **s == HealthState::Up).count();
            let down = components.values().filter(|s| **s == HealthState::Down).count();
            if up == total {
                HealthState::Up
            } else if down == total {
                HealthState::Down
            } else {
                HealthState::Degraded
            }
        };
        Self { state, components }
    }

    /// The overall verdict.
    #[must_use]
    pub const fn state(&self) -> HealthState {
        self.state
    }

    /// Whether the binary is ready to serve the household (fully up).
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.state == HealthState::Up
    }

    /// The components that are not [`HealthState::Up`], for diagnostics.
    #[must_use]
    pub fn unhealthy(&self) -> Vec<Component> {
        self.components
            .iter()
            .filter(|(_, s)| **s != HealthState::Up)
            .map(|(c, _)| *c)
            .collect()
    }

    /// A grandma-friendly one-line summary (Charter §6.3): plain home-world
    /// language, no component internals.
    #[must_use]
    pub fn summary(&self) -> String {
        match self.state {
            HealthState::Up => "Your home is running fine.".to_string(),
            HealthState::Degraded => {
                "Your home is running, but something needs a look.".to_string()
            }
            HealthState::Down => "Your home is not running right now.".to_string(),
        }
    }
}

impl fmt::Display for HealthState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Up => "up",
            Self::Degraded => "degraded",
            Self::Down => "down",
        };
        f.write_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(c: Component, s: HealthState) -> ComponentHealth {
        ComponentHealth::new(c, s)
    }

    #[test]
    fn no_reports_is_down() {
        let agg = Health::aggregate(&[]);
        assert_eq!(agg.state(), HealthState::Down);
        assert!(!agg.is_ready());
    }

    #[test]
    fn all_up_is_up_and_ready() {
        let reports: Vec<_> = Component::ALL
            .into_iter()
            .map(|c| h(c, HealthState::Up))
            .collect();
        let agg = Health::aggregate(&reports);
        assert_eq!(agg.state(), HealthState::Up);
        assert!(agg.is_ready());
        assert!(agg.unhealthy().is_empty());
    }

    #[test]
    fn one_down_among_up_is_degraded() {
        let agg = Health::aggregate(&[
            h(Component::Orchestration, HealthState::Up),
            h(Component::Core, HealthState::Up),
            h(Component::Cameras, HealthState::Down),
        ]);
        assert_eq!(agg.state(), HealthState::Degraded);
        assert!(!agg.is_ready());
        assert_eq!(agg.unhealthy(), vec![Component::Cameras]);
    }

    #[test]
    fn one_degraded_among_up_is_degraded() {
        let agg = Health::aggregate(&[
            h(Component::Orchestration, HealthState::Up),
            h(Component::Core, HealthState::Degraded),
        ]);
        assert_eq!(agg.state(), HealthState::Degraded);
        assert_eq!(agg.unhealthy(), vec![Component::Core]);
    }

    #[test]
    fn all_down_is_down() {
        let agg = Health::aggregate(&[
            h(Component::Orchestration, HealthState::Down),
            h(Component::Core, HealthState::Down),
        ]);
        assert_eq!(agg.state(), HealthState::Down);
    }

    #[test]
    fn all_degraded_is_degraded_not_down() {
        let agg = Health::aggregate(&[
            h(Component::Orchestration, HealthState::Degraded),
            h(Component::Core, HealthState::Degraded),
        ]);
        assert_eq!(agg.state(), HealthState::Degraded);
    }

    #[test]
    fn later_report_for_same_component_wins() {
        let agg = Health::aggregate(&[
            h(Component::Core, HealthState::Down),
            h(Component::Core, HealthState::Up),
        ]);
        // Single component, now Up -> overall Up.
        assert_eq!(agg.state(), HealthState::Up);
    }

    #[test]
    fn summary_is_grandma_friendly() {
        let banned = ["component", "orchestration", "broker", "pod", "mqtt"];
        for reports in [
            vec![h(Component::Core, HealthState::Up)],
            vec![
                h(Component::Core, HealthState::Up),
                h(Component::Cameras, HealthState::Down),
            ],
            vec![],
        ] {
            let s = Health::aggregate(&reports).summary().to_ascii_lowercase();
            for b in banned {
                assert!(!s.contains(b), "summary leaks jargon `{b}`: {s}");
            }
        }
    }

    #[test]
    fn state_display_strings() {
        assert_eq!(HealthState::Up.to_string(), "up");
        assert_eq!(HealthState::Degraded.to_string(), "degraded");
        assert_eq!(HealthState::Down.to_string(), "down");
    }
}
