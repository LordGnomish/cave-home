//! Start-up ordering: the topological bring-up plan + readiness gating.
//!
//! Given a set of components to start (filtered by node role and add-on flags),
//! this module computes a deterministic topological order honouring
//! [`Component::dependencies`], detects dependency cycles and references to
//! components absent from the requested set, and answers "which components may
//! start now, given the ones already ready?" (readiness gating).
//!
//! The order is deterministic: among components whose dependencies are all
//! satisfied, ties break by [`Component`]'s declaration order so the plan is
//! reproducible across runs (important for testing and for operator trust).

use crate::component::Component;
use core::fmt;
use std::collections::BTreeSet;

/// A computed, dependency-respecting start order over a requested component set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BringUpPlan {
    order: Vec<Component>,
}

impl BringUpPlan {
    /// Compute the bring-up order for `requested`.
    ///
    /// Duplicates in `requested` are collapsed. The result lists every
    /// requested component exactly once, with each component appearing only
    /// after all of its (requested) dependencies.
    ///
    /// # Errors
    /// - [`OrderError::MissingDependency`] if a requested component depends on a
    ///   component that is **not** in `requested` (you cannot start the
    ///   apiserver without also requesting kine).
    /// - [`OrderError::Cycle`] if the dependency edges among the requested set
    ///   form a cycle (cannot happen with the built-in graph, but the algorithm
    ///   detects it for any graph).
    pub fn compute(requested: &[Component]) -> Result<Self, OrderError> {
        Self::compute_with_external(requested, &[])
    }

    /// Like [`compute`](Self::compute), but with a set of prerequisites that are
    /// satisfied **externally** — i.e. provided by some node *other than this
    /// one* and therefore not started locally.
    ///
    /// This is what makes an **agent** plannable: an agent runs the kubelet,
    /// whose contract requires a reachable apiserver, but the apiserver lives on
    /// the remote control-plane node. Passing `external = &[Component::Apiserver]`
    /// tells the planner that prerequisite is met without requiring it in the
    /// local set. A server passes no externals — it hosts everything itself.
    ///
    /// # Errors
    /// Same as [`compute`](Self::compute), except a dependency that is listed in
    /// `external` is *not* treated as missing.
    pub fn compute_with_external(
        requested: &[Component],
        external: &[Component],
    ) -> Result<Self, OrderError> {
        Self::compute_with(requested, external, |c| c.dependencies().to_vec())
    }

    /// Core ordering algorithm, parameterised by the dependency function and the
    /// externally-satisfied prerequisites.
    ///
    /// The public entry points supply the real, acyclic K3s graph. The `deps`
    /// parameter exists so the cycle-detection path can be exercised with a
    /// deliberately-cyclic edge function in tests — the built-in graph can never
    /// form a cycle, so there would otherwise be no honest way to test that
    /// branch.
    fn compute_with<F>(
        requested: &[Component],
        external: &[Component],
        deps: F,
    ) -> Result<Self, OrderError>
    where
        F: Fn(Component) -> Vec<Component>,
    {
        // Deterministic, de-duplicated working set.
        let set: BTreeSet<Component> = requested.iter().copied().collect();
        let external: BTreeSet<Component> = external.iter().copied().collect();

        // 1. Every dependency of a requested component must be satisfied either
        //    locally (in `set`) or externally (provided by another node).
        for &c in &set {
            for dep in deps(c) {
                if !set.contains(&dep) && !external.contains(&dep) {
                    return Err(OrderError::MissingDependency {
                        component: c,
                        missing: dep,
                    });
                }
            }
        }

        // 2. Kahn-style sweep over the induced subgraph: repeatedly take every
        //    component whose dependencies are all started, in declaration order,
        //    until none remain or progress stalls (a cycle). Externally-provided
        //    prerequisites count as already-started.
        let nodes: Vec<Component> = set.iter().copied().collect();
        let mut order = Vec::with_capacity(nodes.len());
        let mut started: BTreeSet<Component> = external;

        while order.len() < nodes.len() {
            let mut progressed = false;
            // Iterate in declaration order for a deterministic tie-break.
            for &c in &Component::ALL {
                if !set.contains(&c) || started.contains(&c) {
                    continue;
                }
                let ready = deps(c).iter().all(|d| started.contains(d));
                if ready {
                    order.push(c);
                    started.insert(c);
                    progressed = true;
                }
            }
            if !progressed {
                // Some requested components are not yet started but none became
                // ready -> a cycle among the remaining set.
                let remaining: Vec<Component> = nodes
                    .iter()
                    .copied()
                    .filter(|c| !started.contains(c))
                    .collect();
                return Err(OrderError::Cycle { remaining });
            }
        }

        Ok(Self { order })
    }

    /// The start order, earliest first.
    #[must_use]
    pub fn order(&self) -> &[Component] {
        &self.order
    }

    /// The reverse of the start order — the safe **shutdown** order. Defined
    /// here so bring-up and tear-down stay in lock-step from one source of
    /// truth. (See also [`crate::shutdown`].)
    #[must_use]
    pub fn shutdown_order(&self) -> Vec<Component> {
        let mut rev = self.order.clone();
        rev.reverse();
        rev
    }

    /// Readiness gating: which still-unstarted components may start *now*, given
    /// the set already reported ready.
    ///
    /// A component is eligible when it is in the plan, not already ready, and
    /// all of its dependencies are in `ready`. Returns them in start order.
    #[must_use]
    pub fn startable_now(&self, ready: &[Component]) -> Vec<Component> {
        let ready_set: BTreeSet<Component> = ready.iter().copied().collect();
        self.order
            .iter()
            .copied()
            .filter(|c| !ready_set.contains(c))
            .filter(|c| c.dependencies().iter().all(|d| ready_set.contains(d)))
            .collect()
    }
}

/// Why a bring-up order could not be produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderError {
    /// A requested component depends on one that was not requested.
    MissingDependency {
        component: Component,
        missing: Component,
    },
    /// The requested components' dependency edges form a cycle.
    Cycle { remaining: Vec<Component> },
}

impl fmt::Display for OrderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingDependency { component, missing } => write!(
                f,
                "component '{}' requires '{}', which was not requested",
                component.id(),
                missing.id()
            ),
            Self::Cycle { remaining } => {
                let ids: Vec<&str> = remaining.iter().map(|c| c.id()).collect();
                write!(f, "dependency cycle among: {}", ids.join(", "))
            }
        }
    }
}

impl std::error::Error for OrderError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Assert `a` appears before `b` in the plan.
    fn before(plan: &BringUpPlan, a: Component, b: Component) {
        let order = plan.order();
        let ia = order.iter().position(|c| *c == a).expect("a present");
        let ib = order.iter().position(|c| *c == b).expect("b present");
        assert!(ia < ib, "{} must start before {}", a.id(), b.id());
    }

    #[test]
    fn full_set_orders_kine_first_and_respects_all_edges() {
        let plan = BringUpPlan::compute(&Component::ALL).expect("acyclic full set");
        assert_eq!(plan.order().len(), 10);
        assert_eq!(plan.order()[0], Component::Kine);
        before(&plan, Component::Kine, Component::Apiserver);
        before(&plan, Component::Apiserver, Component::Scheduler);
        before(&plan, Component::Apiserver, Component::ControllerManager);
        before(&plan, Component::Apiserver, Component::Kubelet);
        before(&plan, Component::Cni, Component::Kubelet);
        before(&plan, Component::Kubelet, Component::KubeProxy);
        before(&plan, Component::Apiserver, Component::Traefik);
        before(&plan, Component::Apiserver, Component::ServiceLb);
        before(&plan, Component::Apiserver, Component::HelmController);
    }

    #[test]
    fn order_is_deterministic() {
        let a = BringUpPlan::compute(&Component::ALL).expect("ok");
        let b = BringUpPlan::compute(&Component::ALL).expect("ok");
        assert_eq!(a.order(), b.order());
    }

    #[test]
    fn duplicates_in_request_are_collapsed() {
        let plan = BringUpPlan::compute(&[Component::Kine, Component::Kine, Component::Apiserver])
            .expect("ok");
        assert_eq!(plan.order(), &[Component::Kine, Component::Apiserver]);
    }

    #[test]
    fn missing_dependency_is_detected() {
        // apiserver without kine.
        let err = BringUpPlan::compute(&[Component::Apiserver]).unwrap_err();
        assert_eq!(
            err,
            OrderError::MissingDependency {
                component: Component::Apiserver,
                missing: Component::Kine,
            }
        );
    }

    #[test]
    fn kubelet_missing_cni_is_detected() {
        let err =
            BringUpPlan::compute(&[Component::Kine, Component::Apiserver, Component::Kubelet])
                .unwrap_err();
        assert_eq!(
            err,
            OrderError::MissingDependency {
                component: Component::Kubelet,
                missing: Component::Cni,
            }
        );
    }

    #[test]
    fn agent_only_set_orders_cni_kubelet_proxy_with_external_apiserver() {
        // The kubelet depends on the apiserver, which on an agent is remote;
        // supply it as an external prerequisite.
        let plan = BringUpPlan::compute_with_external(
            &[Component::Cni, Component::Kubelet, Component::KubeProxy],
            &[Component::Apiserver],
        )
        .expect("ok");
        before(&plan, Component::Cni, Component::Kubelet);
        before(&plan, Component::Kubelet, Component::KubeProxy);
    }

    #[test]
    fn external_prerequisite_satisfies_otherwise_missing_dependency() {
        // Without the external apiserver this is a MissingDependency...
        assert!(BringUpPlan::compute(&[Component::Kubelet, Component::Cni]).is_err());
        // ...with it, the kubelet plans fine.
        let plan = BringUpPlan::compute_with_external(
            &[Component::Kubelet, Component::Cni],
            &[Component::Apiserver],
        )
        .expect("external apiserver satisfies the kubelet dependency");
        assert!(plan.order().contains(&Component::Kubelet));
        // The external apiserver is NOT in the local plan — it's another node's.
        assert!(!plan.order().contains(&Component::Apiserver));
    }

    #[test]
    fn readiness_gating_releases_components_as_deps_become_ready() {
        let plan = BringUpPlan::compute(&Component::ALL).expect("ok");

        // Nothing ready: only the roots (kine, cni) may start.
        let now = plan.startable_now(&[]);
        assert_eq!(now, vec![Component::Kine, Component::Cni]);

        // kine ready: apiserver becomes startable; cni still startable.
        let now = plan.startable_now(&[Component::Kine]);
        assert!(now.contains(&Component::Apiserver));
        assert!(now.contains(&Component::Cni));
        assert!(!now.contains(&Component::Scheduler));

        // kine + apiserver + cni ready: scheduler/controller/kubelet/add-ons.
        let now = plan.startable_now(&[Component::Kine, Component::Apiserver, Component::Cni]);
        assert!(now.contains(&Component::Scheduler));
        assert!(now.contains(&Component::ControllerManager));
        assert!(now.contains(&Component::Kubelet));
        assert!(now.contains(&Component::Traefik));
        // kube-proxy still gated behind the kubelet.
        assert!(!now.contains(&Component::KubeProxy));
    }

    #[test]
    fn shutdown_order_is_exact_reverse_of_bringup() {
        let plan = BringUpPlan::compute(&Component::ALL).expect("ok");
        let mut expected = plan.order().to_vec();
        expected.reverse();
        assert_eq!(plan.shutdown_order(), expected);
        // And kine — the datastore — is the last thing torn down.
        assert_eq!(plan.shutdown_order().last(), Some(&Component::Kine));
    }

    #[test]
    fn cycle_is_detected_with_a_cyclic_edge_function() {
        // The built-in graph is acyclic by construction, so we feed the core
        // algorithm a deliberately-cyclic dependency function: scheduler <->
        // controller-manager depend on each other. Kahn's sweep must stall and
        // report a cycle rather than loop forever or panic.
        let cyclic = |c: Component| -> Vec<Component> {
            match c {
                Component::Scheduler => vec![Component::ControllerManager],
                Component::ControllerManager => vec![Component::Scheduler],
                _ => Vec::new(),
            }
        };
        let err = BringUpPlan::compute_with(
            &[Component::Scheduler, Component::ControllerManager],
            &[],
            cyclic,
        )
        .unwrap_err();
        match err {
            OrderError::Cycle { remaining } => {
                assert!(remaining.contains(&Component::Scheduler));
                assert!(remaining.contains(&Component::ControllerManager));
            }
            other => panic!("expected a cycle, got {other:?}"),
        }
    }

    #[test]
    fn order_error_displays_without_panicking() {
        let e1 = OrderError::MissingDependency {
            component: Component::Apiserver,
            missing: Component::Kine,
        };
        let e2 = OrderError::Cycle {
            remaining: vec![Component::Apiserver, Component::Kine],
        };
        assert!(format!("{e1}").contains("apiserver"));
        assert!(format!("{e2}").contains("cycle"));
    }
}
