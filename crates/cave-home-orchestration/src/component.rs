//! The in-process component model.
//!
//! K3s runs a fixed set of components that, in upstream, are goroutines inside
//! one process. cave-home preserves that single-binary shape (Charter §5,
//! ADR-004): every component below is modelled as **in-process**, never a
//! sidecar or sub-process. This module names the components, declares their
//! start-up dependencies, and provides the single-binary invariant check.
//!
//! The dependency edges encode the documented K3s bring-up contract:
//! the datastore (kine) must be ready before the apiserver; the apiserver must
//! be ready before the scheduler, controller-manager and kubelet; the CNI must
//! be ready before kube-proxy; the optional add-ons (servicelb, traefik,
//! helm-controller) depend on the apiserver. The ordering itself is computed in
//! [`crate::bringup`].

/// One coordinated K3s component.
///
/// The set is fixed: cave-home does not load arbitrary components, so an enum
/// (not a string registry) is the right model and makes the dependency graph
/// total and exhaustively matchable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Component {
    /// kine — the etcd-shim datastore (SQLite/external). Root of the graph.
    Kine,
    /// kube-apiserver — the cluster API. Depends on the datastore.
    Apiserver,
    /// kube-scheduler — places pods. Depends on the apiserver.
    Scheduler,
    /// kube-controller-manager — reconciliation loops. Depends on the apiserver.
    ControllerManager,
    /// The container network interface (flannel). A node-side prerequisite.
    Cni,
    /// kubelet — the node agent. Depends on the apiserver and the CNI.
    Kubelet,
    /// kube-proxy — service networking. Depends on the kubelet.
    KubeProxy,
    /// helm-controller — reconciles bundled `HelmChart` resources. Add-on.
    HelmController,
    /// servicelb (klipper-lb) — bare-metal `LoadBalancer`. Add-on.
    ServiceLb,
    /// traefik — ingress. Optional add-on.
    Traefik,
}

impl Component {
    /// Every component cave-home coordinates, in a stable declaration order.
    /// (Declaration order is *not* the start order; see [`crate::bringup`].)
    pub const ALL: [Self; 10] = [
        Self::Kine,
        Self::Apiserver,
        Self::Scheduler,
        Self::ControllerManager,
        Self::Cni,
        Self::Kubelet,
        Self::KubeProxy,
        Self::HelmController,
        Self::ServiceLb,
        Self::Traefik,
    ];

    /// A stable identifier for diagnostics. These are infrastructure-internal
    /// names; they are never shown to end users (Charter §6.3 — the user sees
    /// "Hub" and "Add-ons", never these).
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Kine => "kine",
            Self::Apiserver => "apiserver",
            Self::Scheduler => "scheduler",
            Self::ControllerManager => "controller-manager",
            Self::Cni => "cni",
            Self::Kubelet => "kubelet",
            Self::KubeProxy => "kube-proxy",
            Self::HelmController => "helm-controller",
            Self::ServiceLb => "servicelb",
            Self::Traefik => "traefik",
        }
    }

    /// The components that must be *ready* before this one may start.
    ///
    /// This is the documented K3s bring-up dependency contract, modelled as a
    /// directed acyclic graph. The edges are intentionally minimal (direct
    /// prerequisites only); transitive prerequisites fall out of the topo sort.
    #[must_use]
    pub const fn dependencies(self) -> &'static [Self] {
        match self {
            // kine and the CNI are roots — nothing in this set precedes them.
            Self::Kine | Self::Cni => &[],
            // The apiserver cannot serve until the datastore answers.
            Self::Apiserver => &[Self::Kine],
            // Control-plane controllers + add-ons are all reconciled through the
            // apiserver, so they share its single prerequisite.
            Self::Scheduler
            | Self::ControllerManager
            | Self::HelmController
            | Self::ServiceLb
            | Self::Traefik => &[Self::Apiserver],
            // The kubelet needs the apiserver to register and the CNI for pod
            // networking.
            Self::Kubelet => &[Self::Apiserver, Self::Cni],
            // kube-proxy programmes service rules once the kubelet is up.
            Self::KubeProxy => &[Self::Kubelet],
        }
    }

    /// Whether this component runs only on control-plane (server) nodes.
    /// Agents run the node-side set (CNI, kubelet, kube-proxy) only.
    #[must_use]
    pub const fn is_control_plane(self) -> bool {
        matches!(
            self,
            Self::Kine
                | Self::Apiserver
                | Self::Scheduler
                | Self::ControllerManager
                | Self::HelmController
                | Self::ServiceLb
                | Self::Traefik
        )
    }
}

/// The single-binary invariant (Charter §5, ADR-004).
///
/// Every component in a cave-home deployment must be **in-process**: no sidecar
/// binary, no sub-process supervisor tree. We model that as a property of the
/// component set — there is exactly one process hosting all of them. This check
/// asserts the model never introduces an out-of-process component.
///
/// Because [`Component`] is a closed enum and *all* its variants are in-process
/// by construction, any set drawn from it satisfies the invariant. The check is
/// here so that if a future variant were ever marked out-of-process, the model
/// would reject it rather than silently break Charter §5.
#[must_use]
pub fn all_in_process(components: &[Component]) -> bool {
    components.iter().all(|c| is_in_process(*c))
}

/// Whether a single component is hosted in the unified binary. Currently total:
/// every modelled component is in-process. Kept as a function so the invariant
/// has one authoritative definition to extend.
#[must_use]
pub const fn is_in_process(_component: Component) -> bool {
    // ADR-004: the orchestration layer sits *inside* the unified binary; the
    // §5 carve-out was explicitly not taken. No variant is a sidecar.
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_lists_every_variant_once() {
        assert_eq!(Component::ALL.len(), 10);
        let mut seen = Component::ALL.to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 10, "ALL must contain no duplicates");
    }

    #[test]
    fn ids_are_unique_and_nonempty() {
        let mut ids: Vec<&str> = Component::ALL.iter().map(|c| c.id()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before, "component ids must be unique");
        assert!(Component::ALL.iter().all(|c| !c.id().is_empty()));
    }

    #[test]
    fn roots_have_no_dependencies() {
        assert!(Component::Kine.dependencies().is_empty());
        assert!(Component::Cni.dependencies().is_empty());
    }

    #[test]
    fn apiserver_depends_only_on_datastore() {
        assert_eq!(Component::Apiserver.dependencies(), &[Component::Kine]);
    }

    #[test]
    fn kubelet_depends_on_apiserver_and_cni() {
        let deps = Component::Kubelet.dependencies();
        assert!(deps.contains(&Component::Apiserver));
        assert!(deps.contains(&Component::Cni));
    }

    #[test]
    fn control_plane_classification_matches_role_split() {
        assert!(Component::Apiserver.is_control_plane());
        assert!(Component::Kine.is_control_plane());
        assert!(!Component::Kubelet.is_control_plane());
        assert!(!Component::Cni.is_control_plane());
        assert!(!Component::KubeProxy.is_control_plane());
    }

    #[test]
    fn single_binary_invariant_holds_for_full_set() {
        assert!(all_in_process(&Component::ALL));
        assert!(all_in_process(&[]));
        assert!(is_in_process(Component::Traefik));
    }

    #[test]
    fn dependencies_reference_only_known_components() {
        // Every declared edge must point at a component that appears in ALL —
        // i.e. the graph is closed over the enum.
        for c in Component::ALL {
            for dep in c.dependencies() {
                assert!(Component::ALL.contains(dep));
                assert_ne!(*dep, c, "a component cannot depend on itself");
            }
        }
    }
}
