//! Graceful shutdown ordering.
//!
//! Tearing a node down safely is the bring-up order in reverse: stop the things
//! that *depend* on a component before stopping the component itself, so nothing
//! is left talking to a vanished dependency. Concretely, kube-proxy stops before
//! the kubelet; the control-plane controllers and add-ons stop before the
//! apiserver; the apiserver stops before kine (the datastore is torn down last,
//! after everything that reads or writes cluster state has quiesced).
//!
//! This is a thin, well-tested wrapper over [`BringUpPlan`] so the two halves of
//! the lifecycle can never drift: there is exactly one ordering, and shutdown is
//! its reverse.

use crate::bringup::{BringUpPlan, OrderError};
use crate::component::Component;

/// Compute the graceful shutdown order for a requested component set.
///
/// The order is the exact reverse of the bring-up order, so the dependency
/// invariant holds in reverse: a component is only stopped after every
/// component that depends on it has already been stopped.
///
/// # Errors
/// Propagates [`OrderError`] from bring-up planning (missing dependency / cycle):
/// a set you cannot start is also a set you cannot define a tear-down for.
pub fn shutdown_order(requested: &[Component]) -> Result<Vec<Component>, OrderError> {
    shutdown_order_with_external(requested, &[])
}

/// Like [`shutdown_order`], but for a node with externally-provided
/// prerequisites.
///
/// An agent's component set depends on the remote apiserver; those external
/// prerequisites are not torn down here — they belong to another node.
///
/// # Errors
/// Propagates [`OrderError`] from bring-up planning.
pub fn shutdown_order_with_external(
    requested: &[Component],
    external: &[Component],
) -> Result<Vec<Component>, OrderError> {
    Ok(BringUpPlan::compute_with_external(requested, external)?.shutdown_order())
}

/// Whether `order` is a valid shutdown order for `requested`.
///
/// Valid means: every requested component appears exactly once, and no component
/// is stopped before something that depends on it. This is an executable
/// specification of the tear-down safety property.
#[must_use]
pub fn is_safe_shutdown(requested: &[Component], order: &[Component]) -> bool {
    use std::collections::BTreeSet;
    let want: BTreeSet<Component> = requested.iter().copied().collect();
    let got: BTreeSet<Component> = order.iter().copied().collect();
    if want != got || order.len() != want.len() {
        return false;
    }
    // For each component, all of its (in-set) dependents must come earlier.
    for (i, &c) in order.iter().enumerate() {
        for &later in &order[i + 1..] {
            // `later` is stopped after `c`. That is unsafe only if `later`
            // depends on `c` (we'd be stopping a still-needed dependency first).
            if later.dependencies().contains(&c) {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_set_shutdown_stops_kine_last() {
        let order = shutdown_order(&Component::ALL).expect("ok");
        assert_eq!(order.len(), 10);
        assert_eq!(order.last(), Some(&Component::Kine));
        // The first thing stopped depends on something (a leaf of bring-up).
        assert!(!order[0].dependencies().is_empty() || order[0] == Component::KubeProxy);
    }

    #[test]
    fn shutdown_is_reverse_of_bringup() {
        let mut bring = BringUpPlan::compute(&Component::ALL)
            .expect("ok")
            .order()
            .to_vec();
        let down = shutdown_order(&Component::ALL).expect("ok");
        bring.reverse();
        assert_eq!(down, bring);
    }

    #[test]
    fn proxy_stops_before_kubelet_and_apiserver_before_kine() {
        let order = shutdown_order(&Component::ALL).expect("ok");
        let pos = |c: Component| order.iter().position(|x| *x == c).expect("present");
        assert!(pos(Component::KubeProxy) < pos(Component::Kubelet));
        assert!(pos(Component::Apiserver) < pos(Component::Kine));
        assert!(pos(Component::Scheduler) < pos(Component::Apiserver));
        assert!(pos(Component::Traefik) < pos(Component::Apiserver));
    }

    #[test]
    fn safety_predicate_accepts_the_computed_order() {
        let order = shutdown_order(&Component::ALL).expect("ok");
        assert!(is_safe_shutdown(&Component::ALL, &order));
    }

    #[test]
    fn safety_predicate_rejects_a_bad_order() {
        // Stopping kine before the apiserver leaves the apiserver talking to a
        // dead datastore -> unsafe.
        let bad = vec![
            Component::Kine,
            Component::Apiserver,
            Component::Scheduler,
            Component::ControllerManager,
            Component::Cni,
            Component::Kubelet,
            Component::KubeProxy,
            Component::HelmController,
            Component::ServiceLb,
            Component::Traefik,
        ];
        assert!(!is_safe_shutdown(&Component::ALL, &bad));
    }

    #[test]
    fn safety_predicate_rejects_wrong_membership() {
        let order = shutdown_order(&[Component::Kine, Component::Apiserver]).expect("ok");
        // Missing a component from the requested set.
        assert!(!is_safe_shutdown(&Component::ALL, &order));
    }

    #[test]
    fn missing_dependency_propagates_from_planner() {
        let err = shutdown_order(&[Component::Apiserver]).unwrap_err();
        assert!(matches!(err, OrderError::MissingDependency { .. }));
    }

    #[test]
    fn agent_set_shutdown_stops_proxy_first_cni_last() {
        let set = [Component::Cni, Component::Kubelet, Component::KubeProxy];
        // The agent's kubelet depends on a remote apiserver (external).
        let order = shutdown_order_with_external(&set, &[Component::Apiserver]).expect("ok");
        assert_eq!(order.first(), Some(&Component::KubeProxy));
        assert_eq!(order.last(), Some(&Component::Cni));
        assert!(is_safe_shutdown(&set, &order));
    }
}
