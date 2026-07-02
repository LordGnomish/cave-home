// SPDX-License-Identifier: Apache-2.0
//! ServiceLB controller observability tests (RED until `metrics` lands).
//!
//! The controller exposes the operational signals an operator watches: how many
//! LoadBalancer Services exist, how many are programmed (have a svclb DaemonSet)
//! vs pending (host-port conflict) vs invalid, how many actually publish an
//! ingress IP ("endpoint health"), the svclb DaemonSet (pod-set) count to apply /
//! delete, and the host-port conflict count. These are computed purely from a
//! `Reconciliation` and rendered as Prometheus text exposition.

use std::collections::BTreeMap;
use std::net::IpAddr;

use cave_home_klipper_lb_rs::controller::{reconcile, ReconcileContext, ServiceLbMetrics};
use cave_home_klipper_lb_rs::node::Node;
use cave_home_klipper_lb_rs::service::{
    ExternalTrafficPolicy, LoadBalancerService, Protocol, ServicePort,
};

fn ip(s: &str) -> IpAddr {
    s.parse().expect("test ip")
}

fn svc(name: &str, p: u16, np: u16) -> LoadBalancerService {
    LoadBalancerService {
        namespace: "default".to_owned(),
        name: name.to_owned(),
        load_balancer_ips: vec![],
        ports: vec![ServicePort {
            name: "http".to_owned(),
            protocol: Protocol::Tcp,
            port: p,
            node_port: np,
        }],
        external_traffic_policy: ExternalTrafficPolicy::Cluster,
        node_selector: BTreeMap::new(),
    }
}

#[test]
fn empty_reconcile_has_zero_metrics() {
    let m = reconcile(&[], &[], &ReconcileContext::default()).metrics();
    assert_eq!(m, ServiceLbMetrics::default());
    assert_eq!(m.services_total, 0);
    assert_eq!(m.daemonsets_desired, 0);
}

#[test]
fn metrics_count_programmed_pending_invalid() {
    // web(80) programmed; blog(80) pending (conflict); broken(nodePort 0) invalid.
    let mut broken = svc("broken", 8080, 0);
    broken.ports[0].node_port = 0;
    let services = vec![svc("web", 80, 30080), svc("blog", 80, 30081), broken];
    let nodes = vec![Node::new("n1").with_internal_ip(ip("10.0.0.1"))];

    let m = reconcile(&services, &nodes, &ReconcileContext::default()).metrics();
    assert_eq!(m.services_total, 3);
    assert_eq!(m.services_programmed, 1);
    assert_eq!(m.services_pending, 1);
    assert_eq!(m.services_invalid, 1);
    assert_eq!(m.host_port_conflicts, 1);
    // One DaemonSet to apply (svclb-web), none to delete.
    assert_eq!(m.daemonsets_desired, 1);
    assert_eq!(m.daemonsets_apply, 1);
    assert_eq!(m.daemonsets_delete, 0);
}

#[test]
fn published_counts_only_programmed_services_with_an_ingress_ip() {
    // web has a node with an IP -> published. api has NO node IP -> programmed but
    // not published (status still pending).
    let services = vec![svc("web", 80, 30080), svc("api", 8080, 30090)];
    let nodes = vec![Node::new("n1").with_internal_ip(ip("10.0.0.1"))];
    let m = reconcile(&services, &nodes, &ReconcileContext::default()).metrics();
    assert_eq!(m.services_programmed, 2);
    assert_eq!(m.services_published, 2);

    // With NO node addresses at all, nothing publishes.
    let m2 = reconcile(&services, &[Node::new("n1")], &ReconcileContext::default()).metrics();
    assert_eq!(m2.services_programmed, 2);
    assert_eq!(m2.services_published, 0);
}

#[test]
fn delete_count_tracks_orphans() {
    let ctx = ReconcileContext::default()
        .with_existing_daemonsets(["svclb-a".to_owned(), "svclb-b".to_owned()]);
    let m = reconcile(&[], &[], &ctx).metrics();
    assert_eq!(m.daemonsets_delete, 2);
    assert_eq!(m.daemonsets_desired, 0);
}

#[test]
fn prometheus_exposition_lists_the_gauges() {
    let services = vec![svc("web", 80, 30080), svc("blog", 80, 30081)];
    let nodes = vec![Node::new("n1").with_internal_ip(ip("10.0.0.1"))];
    let text = reconcile(&services, &nodes, &ReconcileContext::default())
        .metrics()
        .to_prometheus();

    // Prometheus text exposition: HELP/TYPE + a value line per gauge.
    assert!(text.contains("# TYPE servicelb_services_total gauge"));
    assert!(text.contains("servicelb_services_total 2"));
    assert!(text.contains("servicelb_services_programmed 1"));
    assert!(text.contains("servicelb_services_pending 1"));
    assert!(text.contains("servicelb_services_published 1"));
    assert!(text.contains("servicelb_daemonsets_desired 1"));
    assert!(text.contains("servicelb_host_port_conflicts_total 1"));
    // Every non-comment line must be "metric value" (parseable).
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(parts.len(), 2, "bad exposition line: {line:?}");
        assert!(parts[1].parse::<u64>().is_ok(), "non-numeric value: {line:?}");
    }
}
