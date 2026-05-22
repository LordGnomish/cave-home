// SPDX-License-Identifier: Apache-2.0
//! Hand-ported subset behavioural tests for the `api` module.
//! Mirrors upstream k8s.io/api/core/v1 + k8s.io/api/discovery/v1 type usage.

use cave_home_kube_proxy_rs::api::{
    Endpoint, EndpointConditions, EndpointPort, EndpointSlice, NamespacedName, Protocol, Service,
    ServicePort, ServicePortName, ServiceType, WatchEvent,
};

#[test]
fn namespaced_name_string_formats_as_ns_slash_name() {
    // Upstream: k8s.io/apimachinery/pkg/types.NamespacedName.String()
    let nn = NamespacedName::new("ns1", "svc1");
    assert_eq!(nn.to_string(), "ns1/svc1");
}

#[test]
fn service_port_name_string_includes_port_suffix() {
    // Upstream: pkg/proxy/types.go ServicePortName.String() / fmtPortName
    let spn = ServicePortName {
        namespaced_name: NamespacedName::new("ns1", "svc1"),
        port: "p80".into(),
        protocol: Protocol::Tcp,
    };
    assert_eq!(spn.to_string(), "ns1/svc1:p80");
}

#[test]
fn service_port_name_string_omits_empty_port() {
    let spn = ServicePortName {
        namespaced_name: NamespacedName::new("ns1", "svc1"),
        port: String::new(),
        protocol: Protocol::Tcp,
    };
    assert_eq!(spn.to_string(), "ns1/svc1");
}

#[test]
fn protocol_lowercase_matches_upstream() {
    // Upstream uses strings.ToLower(string(protocol)) when feeding into hash.
    assert_eq!(Protocol::Tcp.lowercase(), "tcp");
    assert_eq!(Protocol::Udp.lowercase(), "udp");
    assert_eq!(Protocol::Sctp.lowercase(), "sctp");
}

#[test]
fn service_should_skip_when_clusterip_none() {
    // Upstream: pkg/proxy/util/utils.go ShouldSkipService
    let svc = Service {
        metadata: NamespacedName::new("ns1", "svc1"),
        cluster_ip: "None".into(),
        ports: Vec::new(),
        type_: ServiceType::ClusterIP,
    };
    assert!(svc.should_skip());
}

#[test]
fn service_should_skip_when_clusterip_empty() {
    let svc = Service {
        metadata: NamespacedName::new("ns1", "svc1"),
        cluster_ip: String::new(),
        ports: Vec::new(),
        type_: ServiceType::ClusterIP,
    };
    assert!(svc.should_skip());
}

#[test]
fn service_should_skip_when_externalname_type() {
    let svc = Service {
        metadata: NamespacedName::new("ns1", "svc1"),
        cluster_ip: "10.0.0.1".into(),
        ports: Vec::new(),
        type_: ServiceType::ExternalName,
    };
    assert!(svc.should_skip());
}

#[test]
fn service_kept_for_clusterip_with_valid_ip() {
    let svc = Service {
        metadata: NamespacedName::new("ns1", "svc1"),
        cluster_ip: "10.0.0.1".into(),
        ports: Vec::new(),
        type_: ServiceType::ClusterIP,
    };
    assert!(!svc.should_skip());
}

#[test]
fn endpoint_is_ready_when_conditions_ready_true() {
    // Upstream: discovery/v1 EndpointConditions.Ready (default true if nil).
    let e = Endpoint {
        addresses: vec!["10.180.0.1".into()],
        conditions: EndpointConditions { ready: Some(true), serving: None, terminating: None },
    };
    assert!(e.is_ready());
}

#[test]
fn endpoint_not_ready_when_condition_false() {
    let e = Endpoint {
        addresses: vec!["10.180.0.1".into()],
        conditions: EndpointConditions { ready: Some(false), serving: None, terminating: None },
    };
    assert!(!e.is_ready());
}

#[test]
fn endpoint_default_unset_ready_treated_as_ready() {
    // Upstream: nil ready pointer means "ready" — defaulting behaviour.
    let e = Endpoint {
        addresses: vec!["10.180.0.1".into()],
        conditions: EndpointConditions { ready: None, serving: None, terminating: None },
    };
    assert!(e.is_ready());
}

#[test]
fn endpointslice_carries_ports_endpoints_and_service_label() {
    let slice = EndpointSlice {
        metadata: NamespacedName::new("ns1", "svc1-abc"),
        service_name: "svc1".into(),
        ports: vec![EndpointPort { name: "p80".into(), port: 80, protocol: Protocol::Tcp }],
        endpoints: vec![Endpoint {
            addresses: vec!["10.180.0.1".into()],
            conditions: EndpointConditions { ready: Some(true), serving: None, terminating: None },
        }],
    };
    assert_eq!(slice.service_namespaced_name().to_string(), "ns1/svc1");
    assert_eq!(slice.ports.len(), 1);
    assert_eq!(slice.endpoints.len(), 1);
}

#[test]
fn watch_event_variants_carry_objects() {
    let svc = Service {
        metadata: NamespacedName::new("ns1", "svc1"),
        cluster_ip: "10.20.30.41".into(),
        ports: vec![ServicePort { name: "p80".into(), port: 80, protocol: Protocol::Tcp }],
        type_: ServiceType::ClusterIP,
    };
    let added = WatchEvent::ServiceAdded(svc.clone());
    let modified = WatchEvent::ServiceModified(svc.clone());
    let deleted = WatchEvent::ServiceDeleted(svc);
    matches!(added, WatchEvent::ServiceAdded(_));
    matches!(modified, WatchEvent::ServiceModified(_));
    matches!(deleted, WatchEvent::ServiceDeleted(_));
}
