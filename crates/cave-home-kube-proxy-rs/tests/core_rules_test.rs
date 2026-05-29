// SPDX-License-Identifier: Apache-2.0
//! ProxyRule generation tests: ClusterIP / NodePort / LoadBalancer frontends,
//! multi-port services, sessionAffinity mode, zero-endpoint REJECT, headless /
//! ExternalName skipping, determinism, and the incremental rule diff.

use std::net::IpAddr;

use cave_home_kube_proxy_rs::core::model::{
    Endpoint, EndpointConditions, EndpointPort, EndpointSlice, ExternalTrafficPolicy, Protocol,
    Service, ServicePort, ServiceType, SessionAffinity,
};
use cave_home_kube_proxy_rs::core::rules::{
    build_rules, diff_rules, Frontend, LoadBalanceMode, ProxyRule, RuleAction, ServiceInput,
};
use cave_home_kube_proxy_rs::core::select::NodeContext;

fn ip(s: &str) -> IpAddr {
    s.parse().expect("ip literal")
}

fn ready() -> EndpointConditions {
    EndpointConditions { ready: Some(true), serving: Some(true), terminating: Some(false) }
}

fn one_port(name: &str, port: u16, target: u16, node_port: Option<u16>) -> ServicePort {
    ServicePort { name: name.into(), protocol: Protocol::Tcp, port, target_port: target, node_port }
}

fn service(ty: ServiceType, ports: Vec<ServicePort>) -> Service {
    Service {
        namespace: "ns".into(),
        name: "svc".into(),
        cluster_ip: Some(ip("10.96.0.1")),
        service_type: ty,
        ports,
        session_affinity: SessionAffinity::None,
        external_traffic_policy: ExternalTrafficPolicy::Cluster,
        load_balancer_ips: vec![],
    }
}

fn slice_for(port_name: &str, target: u16, ips: &[&str]) -> EndpointSlice {
    EndpointSlice {
        namespace: "ns".into(),
        service_name: "svc".into(),
        slice_name: format!("svc-{port_name}"),
        ports: vec![EndpointPort { name: port_name.into(), protocol: Protocol::Tcp, port: target }],
        endpoints: ips
            .iter()
            .map(|a| Endpoint {
                addresses: vec![ip(a)],
                conditions: ready(),
                node_name: Some("node-a".into()),
                zone: None,
                hints_for_zones: vec![],
            })
            .collect(),
    }
}

fn build(svc: &Service, slices: &[EndpointSlice]) -> Vec<ProxyRule> {
    let inputs = [ServiceInput { service: svc, slices }];
    let node = NodeContext { node_name: Some("node-a".into()), zone: None };
    build_rules(&inputs, &node)
}

#[test]
fn clusterip_service_emits_single_clusterip_rule() {
    let s = service(ServiceType::ClusterIp, vec![one_port("http", 80, 8080, None)]);
    let sl = slice_for("http", 8080, &["10.1.0.1"]);
    let rules = build(&s, std::slice::from_ref(&sl));
    assert_eq!(rules.len(), 1);
    assert!(matches!(rules[0].frontend, Frontend::ClusterIp { port: 80, .. }));
    match &rules[0].action {
        RuleAction::Forward { backends, mode } => {
            assert_eq!(backends.len(), 1);
            assert_eq!(*mode, LoadBalanceMode::Random);
        }
        RuleAction::Reject => panic!("expected forward"),
    }
}

#[test]
fn multiport_service_emits_one_rule_per_port() {
    let s = service(
        ServiceType::ClusterIp,
        vec![one_port("http", 80, 8080, None), one_port("https", 443, 8443, None)],
    );
    let sl1 = slice_for("http", 8080, &["10.1.0.1"]);
    let sl2 = slice_for("https", 8443, &["10.1.0.1"]);
    let rules = build(&s, &[sl1, sl2]);
    assert_eq!(rules.len(), 2);
    let ports: Vec<u16> = rules
        .iter()
        .filter_map(|r| match r.frontend {
            Frontend::ClusterIp { port, .. } => Some(port),
            _ => None,
        })
        .collect();
    assert!(ports.contains(&80) && ports.contains(&443));
}

#[test]
fn nodeport_service_emits_clusterip_and_nodeport_frontends() {
    let s = service(ServiceType::NodePort, vec![one_port("http", 80, 8080, Some(30080))]);
    let sl = slice_for("http", 8080, &["10.1.0.1"]);
    let rules = build(&s, std::slice::from_ref(&sl));
    assert_eq!(rules.len(), 2);
    assert!(rules.iter().any(|r| matches!(r.frontend, Frontend::ClusterIp { .. })));
    assert!(rules.iter().any(|r| matches!(r.frontend, Frontend::NodePort { port: 30080 })));
}

#[test]
fn loadbalancer_service_emits_clusterip_nodeport_and_lb_frontends() {
    let mut s = service(ServiceType::LoadBalancer, vec![one_port("http", 80, 8080, Some(30080))]);
    s.load_balancer_ips = vec![ip("203.0.113.10")];
    let sl = slice_for("http", 8080, &["10.1.0.1"]);
    let rules = build(&s, std::slice::from_ref(&sl));
    assert_eq!(rules.len(), 3);
    assert!(rules.iter().any(|r| matches!(r.frontend, Frontend::ClusterIp { .. })));
    assert!(rules.iter().any(|r| matches!(r.frontend, Frontend::NodePort { .. })));
    assert!(rules
        .iter()
        .any(|r| matches!(&r.frontend, Frontend::LoadBalancer { ip, port: 80 } if *ip == self::ip("203.0.113.10"))));
}

#[test]
fn session_affinity_clientip_sets_sticky_mode() {
    let mut s = service(ServiceType::ClusterIp, vec![one_port("http", 80, 8080, None)]);
    s.session_affinity = SessionAffinity::ClientIp { timeout_seconds: 10_800 };
    let sl = slice_for("http", 8080, &["10.1.0.1"]);
    let rules = build(&s, std::slice::from_ref(&sl));
    match &rules[0].action {
        RuleAction::Forward { mode, .. } => {
            assert_eq!(*mode, LoadBalanceMode::ClientIpSticky { timeout_seconds: 10_800 });
        }
        RuleAction::Reject => panic!("expected forward"),
    }
}

#[test]
fn zero_ready_endpoints_yields_reject_not_blackhole() {
    let s = service(ServiceType::ClusterIp, vec![one_port("http", 80, 8080, None)]);
    let sl = slice_for("http", 8080, &[]); // no endpoints
    let rules = build(&s, std::slice::from_ref(&sl));
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].action, RuleAction::Reject);
}

#[test]
fn local_policy_with_no_local_endpoint_rejects_external_only() {
    // Cluster frontend still forwards; the external (NodePort) frontend, with
    // no local endpoint, rejects.
    let mut s = service(ServiceType::NodePort, vec![one_port("http", 80, 8080, Some(30080))]);
    s.external_traffic_policy = ExternalTrafficPolicy::Local;
    let mut sl = slice_for("http", 8080, &["10.1.0.1"]);
    sl.endpoints[0].node_name = Some("node-elsewhere".into());
    let inputs = [ServiceInput { service: &s, slices: std::slice::from_ref(&sl) }];
    let node = NodeContext { node_name: Some("node-a".into()), zone: None };
    let rules = build_rules(&inputs, &node);

    let cluster = rules.iter().find(|r| matches!(r.frontend, Frontend::ClusterIp { .. })).unwrap();
    let np = rules.iter().find(|r| matches!(r.frontend, Frontend::NodePort { .. })).unwrap();
    assert!(matches!(cluster.action, RuleAction::Forward { .. }));
    assert_eq!(np.action, RuleAction::Reject);
}

#[test]
fn headless_service_is_skipped() {
    let mut s = service(ServiceType::ClusterIp, vec![one_port("http", 80, 8080, None)]);
    s.cluster_ip = None;
    let sl = slice_for("http", 8080, &["10.1.0.1"]);
    assert!(build(&s, std::slice::from_ref(&sl)).is_empty());
}

#[test]
fn externalname_service_is_skipped() {
    let mut s = service(ServiceType::ExternalName, vec![]);
    s.cluster_ip = None;
    assert!(build(&s, &[]).is_empty());
}

#[test]
fn build_is_deterministic() {
    let s = service(
        ServiceType::ClusterIp,
        vec![one_port("http", 80, 8080, None), one_port("https", 443, 8443, None)],
    );
    let sl1 = slice_for("http", 8080, &["10.1.0.2", "10.1.0.1"]);
    let sl2 = slice_for("https", 8443, &["10.1.0.3"]);
    let a = build(&s, &[sl1.clone(), sl2.clone()]);
    let b = build(&s, &[sl2, sl1]);
    assert_eq!(a, b, "rule order independent of input order");
}

// ---- diff ------------------------------------------------------------------

#[test]
fn diff_detects_added_rule() {
    let s = service(ServiceType::ClusterIp, vec![one_port("http", 80, 8080, None)]);
    let sl = slice_for("http", 8080, &["10.1.0.1"]);
    let new = build(&s, std::slice::from_ref(&sl));
    let d = diff_rules(&[], &new);
    assert_eq!(d.added.len(), 1);
    assert!(d.removed.is_empty() && d.changed.is_empty());
}

#[test]
fn diff_detects_removed_rule() {
    let s = service(ServiceType::ClusterIp, vec![one_port("http", 80, 8080, None)]);
    let sl = slice_for("http", 8080, &["10.1.0.1"]);
    let old = build(&s, std::slice::from_ref(&sl));
    let d = diff_rules(&old, &[]);
    assert_eq!(d.removed.len(), 1);
    assert!(d.added.is_empty() && d.changed.is_empty());
}

#[test]
fn diff_detects_backend_change_as_changed() {
    let s = service(ServiceType::ClusterIp, vec![one_port("http", 80, 8080, None)]);
    let old = build(&s, std::slice::from_ref(&slice_for("http", 8080, &["10.1.0.1"])));
    let new = build(&s, std::slice::from_ref(&slice_for("http", 8080, &["10.1.0.1", "10.1.0.2"])));
    let d = diff_rules(&old, &new);
    assert_eq!(d.changed.len(), 1);
    assert!(d.added.is_empty() && d.removed.is_empty());
}

#[test]
fn diff_of_identical_sets_is_empty() {
    let s = service(ServiceType::ClusterIp, vec![one_port("http", 80, 8080, None)]);
    let sl = slice_for("http", 8080, &["10.1.0.1"]);
    let rules = build(&s, std::slice::from_ref(&sl));
    let d = diff_rules(&rules, &rules);
    assert!(d.is_empty());
}

#[test]
fn diff_reject_to_forward_is_a_change() {
    // Endpoints come up: old set rejected, new set forwards -> changed.
    let s = service(ServiceType::ClusterIp, vec![one_port("http", 80, 8080, None)]);
    let old = build(&s, std::slice::from_ref(&slice_for("http", 8080, &[])));
    let new = build(&s, std::slice::from_ref(&slice_for("http", 8080, &["10.1.0.1"])));
    assert_eq!(old[0].action, RuleAction::Reject);
    let d = diff_rules(&old, &new);
    assert_eq!(d.changed.len(), 1);
}

#[test]
fn diff_combined_add_remove_change() {
    // old: svcA(reject), svcB(forward 1) ; new: svcA(forward), svcC(forward)
    let a = service(ServiceType::ClusterIp, vec![one_port("http", 80, 8080, None)]);

    let mut b = a.clone();
    b.name = "svcB".into();
    b.cluster_ip = Some(ip("10.96.0.2"));

    let mut c = a.clone();
    c.name = "svcC".into();
    c.cluster_ip = Some(ip("10.96.0.3"));

    let mut sl_b = slice_for("http", 8080, &["10.2.0.1"]);
    sl_b.service_name = "svcB".into();
    let mut sl_c = slice_for("http", 8080, &["10.3.0.1"]);
    sl_c.service_name = "svcC".into();

    let node = NodeContext::default();
    let old = build_rules(
        &[
            ServiceInput { service: &a, slices: &[slice_for("http", 8080, &[])] },
            ServiceInput { service: &b, slices: std::slice::from_ref(&sl_b) },
        ],
        &node,
    );
    let new = build_rules(
        &[
            ServiceInput { service: &a, slices: &[slice_for("http", 8080, &["10.1.0.1"])] },
            ServiceInput { service: &c, slices: std::slice::from_ref(&sl_c) },
        ],
        &node,
    );
    let d = diff_rules(&old, &new);
    assert_eq!(d.changed.len(), 1, "svcA reject->forward");
    assert_eq!(d.added.len(), 1, "svcC new");
    assert_eq!(d.removed.len(), 1, "svcB gone");
}
