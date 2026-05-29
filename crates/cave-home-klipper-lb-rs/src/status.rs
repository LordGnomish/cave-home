// SPDX-License-Identifier: Apache-2.0
//! Service `status.loadBalancer.ingress` computation.
//!
//! Once svclb pods are scheduled, K3s publishes the IPs the Service is
//! reachable on back into `status.loadBalancer.ingress`. The rule klipper-lb /
//! ServiceLB follows:
//!
//! * If the Service requested explicit `loadBalancerIP(s)`, those are published
//!   verbatim (the operator pinned them).
//! * Otherwise the published IPs are the addresses of the nodes that actually
//!   run a svclb pod — external IP where a node has one, else internal IP.
//! * `externalTrafficPolicy: Local` narrows the publishing set to nodes that
//!   also run a *ready backing pod* for the Service (so traffic isn't sent to a
//!   node that would drop it for lack of a local endpoint).
//!
//! This module computes that publish set. It is a behavioural reimplementation
//! of the documented status flow.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::net::IpAddr;

use crate::node::{select_nodes, Node};
use crate::service::{ExternalTrafficPolicy, LoadBalancerService};

/// Compute the `status.loadBalancer.ingress` IP set for a Service.
///
/// * `svc`             — the Service being published.
/// * `nodes`           — the current cluster nodes.
/// * `nodes_with_pod`  — names of nodes that run a *ready backing pod* for this
///   Service; only consulted when the Service is `externalTrafficPolicy:
///   Local`.
///
/// Returns a de-duplicated, sorted list of IPs. An explicit `loadBalancerIP`
/// short-circuits node selection entirely.
#[must_use]
pub fn compute_ingress_ips(
    svc: &LoadBalancerService,
    nodes: &[Node],
    nodes_with_pod: &BTreeSet<String>,
) -> Vec<IpAddr> {
    // Operator-pinned IPs win outright.
    if !svc.load_balancer_ips.is_empty() {
        let mut ips: Vec<IpAddr> = svc.load_balancer_ips.clone();
        ips.sort_unstable();
        ips.dedup();
        return ips;
    }

    let candidates = select_nodes(nodes, &svc.node_selector);

    let mut ips = BTreeSet::new();
    for n in candidates {
        // For Local ETP, only nodes that host a ready backing pod publish.
        if matches!(svc.external_traffic_policy, ExternalTrafficPolicy::Local)
            && !nodes_with_pod.contains(&n.name)
        {
            continue;
        }
        if let Some(ip) = n.advertise_ip() {
            ips.insert(ip);
        }
    }
    ips.into_iter().collect()
}

/// True iff the Service has at least one ingress IP to publish — i.e. it is no
/// longer `<pending>`. Useful for the controller's status gate.
#[must_use]
pub fn is_published(
    svc: &LoadBalancerService,
    nodes: &[Node],
    nodes_with_pod: &BTreeSet<String>,
) -> bool {
    !compute_ingress_ips(svc, nodes, nodes_with_pod).is_empty()
}

/// Convenience: build a name-set from an iterator of node-name strings.
#[must_use]
pub fn pod_node_set<I, S>(names: I) -> BTreeSet<String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    names.into_iter().map(Into::into).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::{Protocol, ServicePort};

    fn ip(s: &str) -> IpAddr {
        s.parse().expect("test ip")
    }

    fn svc(etp: ExternalTrafficPolicy, lb_ips: Vec<IpAddr>) -> LoadBalancerService {
        LoadBalancerService {
            namespace: "default".to_owned(),
            name: "web".to_owned(),
            load_balancer_ips: lb_ips,
            ports: vec![ServicePort {
                name: "http".to_owned(),
                protocol: Protocol::Tcp,
                port: 80,
                node_port: 30080,
            }],
            external_traffic_policy: etp,
            node_selector: BTreeMap::new(),
        }
    }

    #[test]
    fn explicit_lb_ip_is_published_verbatim() {
        let s = svc(ExternalTrafficPolicy::Cluster, vec![ip("192.168.1.50")]);
        let nodes = vec![Node::new("n1").with_internal_ip(ip("10.0.0.1"))];
        let got = compute_ingress_ips(&s, &nodes, &BTreeSet::new());
        assert_eq!(got, vec![ip("192.168.1.50")]);
    }

    #[test]
    fn cluster_etp_publishes_all_eligible_node_ips() {
        let s = svc(ExternalTrafficPolicy::Cluster, vec![]);
        let nodes = vec![
            Node::new("n1").with_internal_ip(ip("10.0.0.1")),
            Node::new("n2").with_internal_ip(ip("10.0.0.2")),
        ];
        let got = compute_ingress_ips(&s, &nodes, &BTreeSet::new());
        assert_eq!(got, vec![ip("10.0.0.1"), ip("10.0.0.2")]);
    }

    #[test]
    fn external_ip_preferred_over_internal() {
        let s = svc(ExternalTrafficPolicy::Cluster, vec![]);
        let nodes = vec![Node::new("n1")
            .with_internal_ip(ip("10.0.0.1"))
            .with_external_ip(ip("1.2.3.4"))];
        let got = compute_ingress_ips(&s, &nodes, &BTreeSet::new());
        assert_eq!(got, vec![ip("1.2.3.4")]);
    }

    #[test]
    fn cluster_etp_ignores_pod_placement() {
        // Cluster ETP publishes every node even with no local backing pod.
        let s = svc(ExternalTrafficPolicy::Cluster, vec![]);
        let nodes = vec![
            Node::new("n1").with_internal_ip(ip("10.0.0.1")),
            Node::new("n2").with_internal_ip(ip("10.0.0.2")),
        ];
        let got = compute_ingress_ips(&s, &nodes, &pod_node_set(["n1"]));
        assert_eq!(got, vec![ip("10.0.0.1"), ip("10.0.0.2")]);
    }

    #[test]
    fn local_etp_publishes_only_nodes_with_a_ready_pod() {
        let s = svc(ExternalTrafficPolicy::Local, vec![]);
        let nodes = vec![
            Node::new("n1").with_internal_ip(ip("10.0.0.1")),
            Node::new("n2").with_internal_ip(ip("10.0.0.2")),
            Node::new("n3").with_internal_ip(ip("10.0.0.3")),
        ];
        let got = compute_ingress_ips(&s, &nodes, &pod_node_set(["n1", "n3"]));
        assert_eq!(got, vec![ip("10.0.0.1"), ip("10.0.0.3")]);
    }

    #[test]
    fn local_etp_with_no_backing_pod_publishes_nothing() {
        let s = svc(ExternalTrafficPolicy::Local, vec![]);
        let nodes = vec![Node::new("n1").with_internal_ip(ip("10.0.0.1"))];
        let got = compute_ingress_ips(&s, &nodes, &BTreeSet::new());
        assert!(got.is_empty());
        assert!(!is_published(&s, &nodes, &BTreeSet::new()));
    }

    #[test]
    fn cordoned_node_is_not_published() {
        let s = svc(ExternalTrafficPolicy::Cluster, vec![]);
        let nodes = vec![
            Node::new("n1").with_internal_ip(ip("10.0.0.1")),
            Node::new("n2")
                .with_internal_ip(ip("10.0.0.2"))
                .schedulable(false),
        ];
        let got = compute_ingress_ips(&s, &nodes, &BTreeSet::new());
        assert_eq!(got, vec![ip("10.0.0.1")]);
    }

    #[test]
    fn node_without_any_ip_is_skipped() {
        let s = svc(ExternalTrafficPolicy::Cluster, vec![]);
        let nodes = vec![Node::new("n1"), Node::new("n2").with_internal_ip(ip("10.0.0.2"))];
        let got = compute_ingress_ips(&s, &nodes, &BTreeSet::new());
        assert_eq!(got, vec![ip("10.0.0.2")]);
    }

    #[test]
    fn node_selector_narrows_published_nodes() {
        let mut s = svc(ExternalTrafficPolicy::Cluster, vec![]);
        s.node_selector
            .insert("svccontroller.k3s.cattle.io/lbpool".to_owned(), "pool-a".to_owned());
        let nodes = vec![
            Node::new("n1")
                .with_internal_ip(ip("10.0.0.1"))
                .with_label("svccontroller.k3s.cattle.io/lbpool", "pool-a"),
            Node::new("n2").with_internal_ip(ip("10.0.0.2")),
        ];
        let got = compute_ingress_ips(&s, &nodes, &BTreeSet::new());
        assert_eq!(got, vec![ip("10.0.0.1")]);
    }

    #[test]
    fn duplicate_explicit_lb_ips_are_deduped() {
        let s = svc(
            ExternalTrafficPolicy::Cluster,
            vec![ip("192.168.1.50"), ip("192.168.1.50")],
        );
        let got = compute_ingress_ips(&s, &[], &BTreeSet::new());
        assert_eq!(got, vec![ip("192.168.1.50")]);
    }

    #[test]
    fn is_published_true_when_a_node_advertises() {
        let s = svc(ExternalTrafficPolicy::Cluster, vec![]);
        let nodes = vec![Node::new("n1").with_internal_ip(ip("10.0.0.1"))];
        assert!(is_published(&s, &nodes, &BTreeSet::new()));
    }
}
