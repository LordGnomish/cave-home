//! Setup-order resolution: a topological sort of integrations by their
//! dependencies and after-dependencies.
//!
//! Before the engine sets entries up it must order their integrations so a
//! dependency is always loaded before the thing that needs it. This is a plain
//! Kahn-style topological sort over the dependency graph, with two honest
//! failure modes the engine must surface rather than loop on:
//!
//! - a **missing dependency** (something depends on an integration that isn't
//!   registered), and
//! - a **dependency cycle** (A needs B needs A — unorderable).
//!
//! Hard dependencies and after-dependencies are both edges for ordering
//! purposes; the difference is that a *missing* hard dependency is an error,
//! while a missing after-dependency is simply ignored (HA semantics: "after, if
//! present").

use crate::capability::HubCapabilities;
use crate::config_entry::ConfigEntry;
use crate::integration::{Integration, Registry};

/// Why a setup order could not be produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// `domain` hard-depends on `missing`, which is not registered.
    MissingDependency { domain: String, missing: String },
    /// A dependency cycle exists among these domains (sorted for stability).
    Cycle { domains: Vec<String> },
}

impl core::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MissingDependency { domain, missing } => {
                write!(f, "{domain} needs {missing}, which is not installed")
            }
            Self::Cycle { domains } => {
                write!(f, "these depend on each other in a loop: {}", domains.join(", "))
            }
        }
    }
}

impl std::error::Error for ResolveError {}

/// Produce a setup order for the given domains: every domain appears after all
/// of its (present) dependencies and after-dependencies.
///
/// Only the listed `domains` are ordered; their edges to integrations *outside*
/// the set are honoured for validation (a missing hard dependency is an error)
/// but do not pull extra domains into the result. Ties are broken by the
/// registration order in `registry` for deterministic output.
///
/// # Errors
/// Returns [`ResolveError::MissingDependency`] for an absent hard dependency,
/// or [`ResolveError::Cycle`] if the requested set cannot be linearised.
pub fn resolve_setup_order(
    registry: &Registry,
    domains: &[&str],
) -> Result<Vec<String>, ResolveError> {
    // Stable index for tie-breaking: registration order.
    let order_of = |d: &str| -> usize {
        registry
            .all()
            .iter()
            .position(|i| i.domain() == d)
            .unwrap_or(usize::MAX)
    };

    let in_set = |d: &str| domains.contains(&d);

    // Validate hard dependencies first: a missing one is a hard error.
    for &d in domains {
        if let Some(integ) = registry.get(d) {
            for dep in integ.dependencies() {
                if registry.get(dep).is_none() {
                    return Err(ResolveError::MissingDependency {
                        domain: d.to_string(),
                        missing: dep.clone(),
                    });
                }
            }
        }
    }

    // Build edges among the requested set only.
    let edges = |integ: &Integration| -> Vec<String> {
        integ
            .dependencies()
            .iter()
            .chain(integ.after_dependencies().iter())
            .filter(|dep| in_set(dep))
            .cloned()
            .collect::<Vec<_>>()
    };

    // Kahn's algorithm. remaining: domains not yet emitted.
    let mut remaining: Vec<String> = domains.iter().map(|d| (*d).to_string()).collect();
    let mut output: Vec<String> = Vec::with_capacity(remaining.len());

    while !remaining.is_empty() {
        // A node is ready when all its in-set deps are already emitted.
        let mut ready: Vec<String> = remaining
            .iter()
            .filter(|d| {
                registry.get(d).is_none_or(|integ| {
                    edges(integ).iter().all(|dep| output.contains(dep) || !in_set(dep))
                })
            })
            .cloned()
            .collect();

        if ready.is_empty() {
            // Nothing can advance -> the remainder forms a cycle.
            let mut domains = remaining.clone();
            domains.sort();
            return Err(ResolveError::Cycle { domains });
        }

        // Deterministic tie-break by registration order.
        ready.sort_by_key(|d| order_of(d));
        for d in ready {
            output.push(d.clone());
            remaining.retain(|r| r != &d);
        }
    }

    Ok(output)
}

/// Aggregate "what can this hub do" across the entries that are *loaded* and
/// *not disabled*. A disabled or not-yet-loaded entry contributes nothing.
#[must_use]
pub fn hub_capabilities(registry: &Registry, entries: &[ConfigEntry]) -> HubCapabilities {
    let mut hub = HubCapabilities::new();
    for entry in entries {
        if entry.is_disabled() || !entry.state().is_running() {
            continue;
        }
        if let Some(integ) = registry.get(entry.domain()) {
            for &cap in integ.capabilities() {
                hub.add(cap);
            }
        }
    }
    hub
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::Capability;
    use crate::integration::Integration;
    use crate::lifecycle::Transition;

    fn reg() -> Registry {
        let mut r = Registry::new();
        // network <- mqtt <- light; hue after mqtt.
        r.register(Integration::new("network", "Network"));
        r.register(Integration::new("mqtt", "MQTT broker").depends_on("network"));
        r.register(
            Integration::new("light", "Light platform")
                .with_capability(Capability::Light)
                .depends_on("mqtt"),
        );
        r.register(
            Integration::new("hue", "Philips Hue")
                .with_capability(Capability::Light)
                .with_capability(Capability::Sensor)
                .after("mqtt"),
        );
        r
    }

    fn pos(order: &[String], d: &str) -> usize {
        order.iter().position(|x| x == d).expect("present")
    }

    #[test]
    fn dependency_comes_before_dependent() {
        let r = reg();
        let order = resolve_setup_order(&r, &["light", "mqtt", "network"]).expect("order");
        assert_eq!(order.len(), 3);
        assert!(pos(&order, "network") < pos(&order, "mqtt"));
        assert!(pos(&order, "mqtt") < pos(&order, "light"));
    }

    #[test]
    fn after_dependency_orders_when_present() {
        let r = reg();
        let order = resolve_setup_order(&r, &["hue", "mqtt", "network"]).expect("order");
        assert!(pos(&order, "mqtt") < pos(&order, "hue"));
    }

    #[test]
    fn after_dependency_ignored_when_absent() {
        let r = reg();
        // mqtt not in the set; hue's `after` is soft, so this still resolves.
        let order = resolve_setup_order(&r, &["hue"]).expect("order");
        assert_eq!(order, vec!["hue".to_string()]);
    }

    #[test]
    fn missing_hard_dependency_is_an_error() {
        let mut r = Registry::new();
        r.register(Integration::new("light", "Light").depends_on("mqtt"));
        // mqtt is never registered.
        let err = resolve_setup_order(&r, &["light"]).unwrap_err();
        assert_eq!(
            err,
            ResolveError::MissingDependency {
                domain: "light".into(),
                missing: "mqtt".into()
            }
        );
    }

    #[test]
    fn cycle_is_detected_not_looped() {
        let mut r = Registry::new();
        r.register(Integration::new("a", "A").depends_on("b"));
        r.register(Integration::new("b", "B").depends_on("a"));
        let err = resolve_setup_order(&r, &["a", "b"]).unwrap_err();
        assert_eq!(err, ResolveError::Cycle { domains: vec!["a".into(), "b".into()] });
    }

    #[test]
    fn three_node_cycle_is_detected() {
        let mut r = Registry::new();
        r.register(Integration::new("a", "A").depends_on("b"));
        r.register(Integration::new("b", "B").depends_on("c"));
        r.register(Integration::new("c", "C").depends_on("a"));
        let err = resolve_setup_order(&r, &["a", "b", "c"]).unwrap_err();
        assert!(matches!(err, ResolveError::Cycle { .. }));
    }

    #[test]
    fn empty_set_resolves_to_empty_order() {
        let r = reg();
        assert!(resolve_setup_order(&r, &[]).expect("ok").is_empty());
    }

    #[test]
    fn order_is_deterministic_across_runs() {
        let r = reg();
        let a = resolve_setup_order(&r, &["network", "mqtt", "light", "hue"]).expect("a");
        let b = resolve_setup_order(&r, &["hue", "light", "mqtt", "network"]).expect("b");
        assert_eq!(a, b, "tie-break by registration order must be stable");
    }

    #[test]
    fn hub_capabilities_aggregate_loaded_only() {
        let r = reg();
        let mut hue = ConfigEntry::new("hue", "Hue");
        hue.apply(Transition::BeginSetup).expect("s");
        hue.apply(Transition::SetupSucceeded).expect("l");
        // light entry exists but is not loaded -> contributes nothing.
        let light = ConfigEntry::new("light", "Light");
        let hub = hub_capabilities(&r, &[hue, light]);
        assert!(hub.has(Capability::Light));
        assert!(hub.has(Capability::Sensor));
        assert_eq!(hub.len(), 2, "only the loaded hue's caps count");
    }

    #[test]
    fn disabled_entry_contributes_no_capabilities() {
        let r = reg();
        let mut hue = ConfigEntry::new("hue", "Hue");
        hue.apply(Transition::BeginSetup).expect("s");
        hue.apply(Transition::SetupSucceeded).expect("l");
        hue.disable(crate::config_entry::DisabledBy::User);
        let hub = hub_capabilities(&r, &[hue]);
        assert!(hub.is_empty());
    }
}
