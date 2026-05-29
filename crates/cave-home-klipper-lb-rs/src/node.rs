// SPDX-License-Identifier: Apache-2.0
//! Cluster node model + svclb node-selection.
//!
//! klipper-lb (K3s "svclb" / ServiceLB) runs a `DaemonSet` pod per
//! LoadBalancer-type Service; a copy of that pod lands on every node the
//! DaemonSet schedules onto. This module models the nodes and answers the
//! question the DaemonSet controller answers: *which nodes should run the
//! svclb pod for a given Service?*
//!
//! A node is eligible when it is `Ready`, `schedulable` (not cordoned), and
//! matches any `nodeSelector` the Service requested (K3s exposes this through
//! the `svccontroller.k3s.cattle.io/enablelb` flow + the daemonset's own node
//! affinity). This mirrors the documented behaviour, not a verbatim port.

use std::collections::BTreeMap;
use std::net::IpAddr;

/// A cluster node, the svclb-relevant subset of `k8s.io/api/core/v1.Node`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Node {
    /// Node name (`metadata.name`), unique within the cluster.
    pub name: String,
    /// `status.addresses[type=InternalIP]` — the in-cluster routable IP.
    pub internal_ip: Option<IpAddr>,
    /// `status.addresses[type=ExternalIP]` — the publicly routable IP, if any.
    pub external_ip: Option<IpAddr>,
    /// `status.conditions[type=Ready].status == "True"`.
    pub ready: bool,
    /// `!spec.unschedulable` — false when the node is cordoned.
    pub schedulable: bool,
    /// `metadata.labels` — matched against a Service's `nodeSelector`.
    pub labels: BTreeMap<String, String>,
}

impl Node {
    /// Construct a ready + schedulable node with the given name and no labels.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            internal_ip: None,
            external_ip: None,
            ready: true,
            schedulable: true,
            labels: BTreeMap::new(),
        }
    }

    /// Builder: set the internal IP.
    #[must_use]
    pub const fn with_internal_ip(mut self, ip: IpAddr) -> Self {
        self.internal_ip = Some(ip);
        self
    }

    /// Builder: set the external IP.
    #[must_use]
    pub const fn with_external_ip(mut self, ip: IpAddr) -> Self {
        self.external_ip = Some(ip);
        self
    }

    /// Builder: set readiness.
    #[must_use]
    pub const fn ready(mut self, ready: bool) -> Self {
        self.ready = ready;
        self
    }

    /// Builder: set schedulability (false == cordoned).
    #[must_use]
    pub const fn schedulable(mut self, schedulable: bool) -> Self {
        self.schedulable = schedulable;
        self
    }

    /// Builder: add a label.
    #[must_use]
    pub fn with_label(mut self, key: impl Into<String>, val: impl Into<String>) -> Self {
        self.labels.insert(key.into(), val.into());
        self
    }

    /// True iff this node can host *any* svclb pod: ready and schedulable.
    #[must_use]
    pub const fn is_eligible(&self) -> bool {
        self.ready && self.schedulable
    }

    /// True iff every key/value in `selector` is present in this node's labels.
    /// An empty selector matches every node (`nodeSelector: {}` semantics).
    #[must_use]
    pub fn matches_selector(&self, selector: &BTreeMap<String, String>) -> bool {
        selector
            .iter()
            .all(|(k, v)| self.labels.get(k).is_some_and(|got| got == v))
    }

    /// The IP a svclb pod on this node advertises for the Service status: the
    /// external IP when present, otherwise the internal IP. `None` when neither
    /// address is known (such a node cannot publish an ingress IP).
    #[must_use]
    pub fn advertise_ip(&self) -> Option<IpAddr> {
        self.external_ip.or(self.internal_ip)
    }
}

/// Select the nodes a svclb `DaemonSet` should schedule onto for a Service
/// with the given `nodeSelector`.
///
/// A node qualifies when it is ready, schedulable, and matches the selector.
/// Output preserves input order, so callers get a stable, deterministic set
/// that can be diffed across reconciles (node add/remove recompute).
#[must_use]
pub fn select_nodes<'a>(
    nodes: &'a [Node],
    selector: &BTreeMap<String, String>,
) -> Vec<&'a Node> {
    nodes
        .iter()
        .filter(|n| n.is_eligible() && n.matches_selector(selector))
        .collect()
}

/// The names of the selected nodes — a convenience over [`select_nodes`] for
/// diffing the scheduled set between two cluster snapshots.
#[must_use]
pub fn selected_node_names(nodes: &[Node], selector: &BTreeMap<String, String>) -> Vec<String> {
    select_nodes(nodes, selector)
        .into_iter()
        .map(|n| n.name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().expect("test ip literal")
    }

    fn sel(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn ready_schedulable_node_is_eligible() {
        assert!(Node::new("a").is_eligible());
    }

    #[test]
    fn cordoned_node_is_not_eligible() {
        assert!(!Node::new("a").schedulable(false).is_eligible());
    }

    #[test]
    fn not_ready_node_is_not_eligible() {
        assert!(!Node::new("a").ready(false).is_eligible());
    }

    #[test]
    fn empty_selector_matches_any_node() {
        let n = Node::new("a");
        assert!(n.matches_selector(&BTreeMap::new()));
    }

    #[test]
    fn selector_requires_all_pairs() {
        let n = Node::new("a")
            .with_label("disk", "ssd")
            .with_label("zone", "z1");
        assert!(n.matches_selector(&sel(&[("disk", "ssd")])));
        assert!(n.matches_selector(&sel(&[("disk", "ssd"), ("zone", "z1")])));
        assert!(!n.matches_selector(&sel(&[("disk", "ssd"), ("zone", "z2")])));
        assert!(!n.matches_selector(&sel(&[("missing", "x")])));
    }

    #[test]
    fn select_nodes_filters_unready_cordoned_and_unmatched() {
        let nodes = vec![
            Node::new("ready").with_label("role", "lb"),
            Node::new("cordoned").schedulable(false).with_label("role", "lb"),
            Node::new("down").ready(false).with_label("role", "lb"),
            Node::new("nolabel"),
        ];
        let got = selected_node_names(&nodes, &sel(&[("role", "lb")]));
        assert_eq!(got, vec!["ready".to_owned()]);
    }

    #[test]
    fn select_nodes_preserves_input_order() {
        let nodes = vec![Node::new("c"), Node::new("a"), Node::new("b")];
        let got = selected_node_names(&nodes, &BTreeMap::new());
        assert_eq!(got, vec!["c", "a", "b"]);
    }

    #[test]
    fn node_add_recomputes_selected_set() {
        let mut nodes = vec![Node::new("a")];
        assert_eq!(selected_node_names(&nodes, &BTreeMap::new()), vec!["a"]);
        nodes.push(Node::new("b"));
        assert_eq!(
            selected_node_names(&nodes, &BTreeMap::new()),
            vec!["a", "b"]
        );
    }

    #[test]
    fn node_remove_recomputes_selected_set() {
        let mut nodes = vec![Node::new("a"), Node::new("b")];
        nodes.retain(|n| n.name != "a");
        assert_eq!(selected_node_names(&nodes, &BTreeMap::new()), vec!["b"]);
    }

    #[test]
    fn cordoning_a_node_drops_it_from_the_set() {
        let mut nodes = vec![Node::new("a"), Node::new("b")];
        assert_eq!(selected_node_names(&nodes, &BTreeMap::new()).len(), 2);
        nodes[0].schedulable = false;
        assert_eq!(selected_node_names(&nodes, &BTreeMap::new()), vec!["b"]);
    }

    #[test]
    fn advertise_ip_prefers_external_then_internal_then_none() {
        assert_eq!(Node::new("a").advertise_ip(), None);
        assert_eq!(
            Node::new("a").with_internal_ip(ip("10.0.0.1")).advertise_ip(),
            Some(ip("10.0.0.1"))
        );
        assert_eq!(
            Node::new("a")
                .with_internal_ip(ip("10.0.0.1"))
                .with_external_ip(ip("1.2.3.4"))
                .advertise_ip(),
            Some(ip("1.2.3.4"))
        );
    }
}
