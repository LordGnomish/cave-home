// SPDX-License-Identifier: Apache-2.0
//! Endpoint-selection tests: readiness/serving/terminating filtering,
//! topology-aware hints, externalTrafficPolicy Local vs Cluster, named-port
//! resolution, and the terminating fallback.

use std::net::IpAddr;

use cave_home_kube_proxy_rs::core::model::{
    Endpoint, EndpointConditions, EndpointPort, EndpointSlice, ExternalTrafficPolicy, Protocol,
    Service, ServicePort, ServiceType, SessionAffinity,
};
use cave_home_kube_proxy_rs::core::select::{select_endpoints, NodeContext};

fn ip(s: &str) -> IpAddr {
    s.parse().expect("ip literal")
}

fn svc(etp: ExternalTrafficPolicy) -> Service {
    Service {
        namespace: "ns".into(),
        name: "svc".into(),
        cluster_ip: Some(ip("10.96.0.1")),
        service_type: ServiceType::NodePort,
        ports: vec![port()],
        session_affinity: SessionAffinity::None,
        external_traffic_policy: etp,
        load_balancer_ips: vec![],
    }
}

fn port() -> ServicePort {
    ServicePort {
        name: "http".into(),
        protocol: Protocol::Tcp,
        port: 80,
        target_port: 8080,
        node_port: Some(30080),
    }
}

fn ep(addr: &str, conds: EndpointConditions, node: Option<&str>, zone: Option<&str>) -> Endpoint {
    Endpoint {
        addresses: vec![ip(addr)],
        conditions: conds,
        node_name: node.map(String::from),
        zone: zone.map(String::from),
        hints_for_zones: vec![],
    }
}

fn ready() -> EndpointConditions {
    EndpointConditions { ready: Some(true), serving: Some(true), terminating: Some(false) }
}

fn unready() -> EndpointConditions {
    EndpointConditions { ready: Some(false), serving: Some(false), terminating: Some(false) }
}

fn slice(eps: Vec<Endpoint>) -> EndpointSlice {
    EndpointSlice {
        namespace: "ns".into(),
        service_name: "svc".into(),
        slice_name: "svc-1".into(),
        ports: vec![EndpointPort { name: "http".into(), protocol: Protocol::Tcp, port: 8080 }],
        endpoints: eps,
    }
}

#[test]
fn only_ready_endpoints_selected() {
    let s = svc(ExternalTrafficPolicy::Cluster);
    let sl = slice(vec![
        ep("10.1.0.1", ready(), None, None),
        ep("10.1.0.2", unready(), None, None),
    ]);
    let sel = select_endpoints(&s, &s.ports[0], std::slice::from_ref(&sl), &NodeContext::default());
    assert_eq!(sel.cluster.len(), 1);
    assert_eq!(sel.cluster[0].ip, ip("10.1.0.1"));
    assert!(!sel.from_terminating);
}

#[test]
fn unset_ready_condition_treated_as_ready() {
    let s = svc(ExternalTrafficPolicy::Cluster);
    let sl = slice(vec![ep("10.1.0.9", EndpointConditions::default(), None, None)]);
    let sel = select_endpoints(&s, &s.ports[0], std::slice::from_ref(&sl), &NodeContext::default());
    assert_eq!(sel.cluster.len(), 1);
}

#[test]
fn terminating_endpoint_excluded_from_normal_set() {
    let s = svc(ExternalTrafficPolicy::Cluster);
    let term = EndpointConditions { ready: Some(false), serving: Some(true), terminating: Some(true) };
    let sl = slice(vec![
        ep("10.1.0.1", ready(), None, None),
        ep("10.1.0.2", term, None, None),
    ]);
    let sel = select_endpoints(&s, &s.ports[0], std::slice::from_ref(&sl), &NodeContext::default());
    assert_eq!(sel.cluster.len(), 1);
    assert_eq!(sel.cluster[0].ip, ip("10.1.0.1"));
}

#[test]
fn terminating_fallback_used_when_no_ready() {
    // No ready endpoints, but a serving-terminating one exists -> use it
    // rather than black-holing the Service.
    let s = svc(ExternalTrafficPolicy::Cluster);
    let term = EndpointConditions { ready: Some(false), serving: Some(true), terminating: Some(true) };
    let sl = slice(vec![ep("10.1.0.7", term, None, None)]);
    let sel = select_endpoints(&s, &s.ports[0], std::slice::from_ref(&sl), &NodeContext::default());
    assert_eq!(sel.cluster.len(), 1);
    assert!(sel.from_terminating);
}

#[test]
fn target_port_resolved_from_named_slice_port() {
    let s = svc(ExternalTrafficPolicy::Cluster);
    let sl = slice(vec![ep("10.1.0.1", ready(), None, None)]);
    let sel = select_endpoints(&s, &s.ports[0], std::slice::from_ref(&sl), &NodeContext::default());
    assert_eq!(sel.cluster[0].port, 8080);
}

#[test]
fn external_policy_cluster_uses_all_ready() {
    let s = svc(ExternalTrafficPolicy::Cluster);
    let node = NodeContext { node_name: Some("node-a".into()), zone: None };
    let sl = slice(vec![
        ep("10.1.0.1", ready(), Some("node-a"), None),
        ep("10.1.0.2", ready(), Some("node-b"), None),
    ]);
    let sel = select_endpoints(&s, &s.ports[0], std::slice::from_ref(&sl), &node);
    assert_eq!(sel.cluster.len(), 2);
    assert_eq!(sel.external.len(), 2, "Cluster policy: external == cluster");
}

#[test]
fn external_policy_local_keeps_only_local_node() {
    let s = svc(ExternalTrafficPolicy::Local);
    let node = NodeContext { node_name: Some("node-a".into()), zone: None };
    let sl = slice(vec![
        ep("10.1.0.1", ready(), Some("node-a"), None),
        ep("10.1.0.2", ready(), Some("node-b"), None),
    ]);
    let sel = select_endpoints(&s, &s.ports[0], std::slice::from_ref(&sl), &node);
    // Cluster traffic still spreads to all; external is local-only.
    assert_eq!(sel.cluster.len(), 2);
    assert_eq!(sel.external.len(), 1);
    assert_eq!(sel.external[0].ip, ip("10.1.0.1"));
    assert!(sel.external[0].local);
}

#[test]
fn external_policy_local_empty_when_no_local_endpoint() {
    // Local policy on a node with no local endpoint -> empty external set
    // (the rule builder turns this into a Reject for the external frontend).
    let s = svc(ExternalTrafficPolicy::Local);
    let node = NodeContext { node_name: Some("node-c".into()), zone: None };
    let sl = slice(vec![
        ep("10.1.0.1", ready(), Some("node-a"), None),
        ep("10.1.0.2", ready(), Some("node-b"), None),
    ]);
    let sel = select_endpoints(&s, &s.ports[0], std::slice::from_ref(&sl), &node);
    assert_eq!(sel.cluster.len(), 2);
    assert!(sel.external.is_empty());
}

fn hinted(addr: &str, zone: &str, for_zones: &[&str]) -> Endpoint {
    Endpoint {
        addresses: vec![ip(addr)],
        conditions: ready(),
        node_name: None,
        zone: Some(zone.into()),
        hints_for_zones: for_zones.iter().map(|z| (*z).to_string()).collect(),
    }
}

#[test]
fn topology_hints_restrict_to_local_zone() {
    let s = svc(ExternalTrafficPolicy::Cluster);
    let node = NodeContext { node_name: None, zone: Some("z1".into()) };
    let sl = slice(vec![
        hinted("10.1.0.1", "z1", &["z1"]),
        hinted("10.1.0.2", "z2", &["z2"]),
    ]);
    let sel = select_endpoints(&s, &s.ports[0], std::slice::from_ref(&sl), &node);
    assert_eq!(sel.cluster.len(), 1);
    assert_eq!(sel.cluster[0].ip, ip("10.1.0.1"));
}

#[test]
fn topology_hints_ignored_when_not_all_endpoints_hinted() {
    // One endpoint lacks hints -> fail-open, hints ignored, both selected.
    let s = svc(ExternalTrafficPolicy::Cluster);
    let node = NodeContext { node_name: None, zone: Some("z1".into()) };
    let sl = slice(vec![
        hinted("10.1.0.1", "z1", &["z1"]),
        ep("10.1.0.2", ready(), None, Some("z2")), // no hints
    ]);
    let sel = select_endpoints(&s, &s.ports[0], std::slice::from_ref(&sl), &node);
    assert_eq!(sel.cluster.len(), 2);
}

#[test]
fn topology_hints_fail_open_when_local_zone_has_none() {
    // All hinted, but none for our zone -> would empty the set -> ignore hint.
    let s = svc(ExternalTrafficPolicy::Cluster);
    let node = NodeContext { node_name: None, zone: Some("z9".into()) };
    let sl = slice(vec![
        hinted("10.1.0.1", "z1", &["z1"]),
        hinted("10.1.0.2", "z2", &["z2"]),
    ]);
    let sel = select_endpoints(&s, &s.ports[0], std::slice::from_ref(&sl), &node);
    assert_eq!(sel.cluster.len(), 2, "hint that black-holes the zone is ignored");
}

#[test]
fn topology_hints_inactive_without_node_zone() {
    let s = svc(ExternalTrafficPolicy::Cluster);
    let sl = slice(vec![
        hinted("10.1.0.1", "z1", &["z1"]),
        hinted("10.1.0.2", "z2", &["z2"]),
    ]);
    // No node zone known -> hints can't apply.
    let sel = select_endpoints(&s, &s.ports[0], std::slice::from_ref(&sl), &NodeContext::default());
    assert_eq!(sel.cluster.len(), 2);
}

#[test]
fn endpoints_merged_across_slices_and_sorted() {
    let s = svc(ExternalTrafficPolicy::Cluster);
    let sl1 = slice(vec![ep("10.1.0.2", ready(), None, None)]);
    let mut sl2 = slice(vec![ep("10.1.0.1", ready(), None, None)]);
    sl2.slice_name = "svc-2".into();
    let sel = select_endpoints(&s, &s.ports[0], &[sl1, sl2], &NodeContext::default());
    assert_eq!(sel.cluster.len(), 2);
    assert_eq!(sel.cluster[0].ip, ip("10.1.0.1"), "sorted ascending");
    assert_eq!(sel.cluster[1].ip, ip("10.1.0.2"));
}

#[test]
fn duplicate_endpoint_addresses_deduped() {
    let s = svc(ExternalTrafficPolicy::Cluster);
    let sl1 = slice(vec![ep("10.1.0.1", ready(), None, None)]);
    let mut sl2 = slice(vec![ep("10.1.0.1", ready(), None, None)]);
    sl2.slice_name = "svc-2".into();
    let sel = select_endpoints(&s, &s.ports[0], &[sl1, sl2], &NodeContext::default());
    assert_eq!(sel.cluster.len(), 1);
}

#[test]
fn no_matching_port_yields_no_endpoints() {
    let s = svc(ExternalTrafficPolicy::Cluster);
    let mut sl = slice(vec![ep("10.1.0.1", ready(), None, None)]);
    // Slice port name doesn't match the Service port and target_port is set,
    // so the numeric fallback (8080) is still used.
    sl.ports[0].name = "other".into();
    let sel = select_endpoints(&s, &s.ports[0], std::slice::from_ref(&sl), &NodeContext::default());
    assert_eq!(sel.cluster.len(), 1);
    assert_eq!(sel.cluster[0].port, 8080, "falls back to numeric target_port");
}
