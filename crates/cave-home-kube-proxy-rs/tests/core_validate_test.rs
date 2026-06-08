// SPDX-License-Identifier: Apache-2.0
//! Validation tests for the decision core — malformed Services / slices are
//! rejected with a typed error, never a panic.

use std::net::IpAddr;

use cave_home_kube_proxy_rs::core::model::{
    Endpoint, EndpointConditions, EndpointPort, EndpointSlice, ExternalTrafficPolicy, Protocol,
    Service, ServicePort, ServiceType, SessionAffinity,
};
use cave_home_kube_proxy_rs::core::validate::{validate_service, validate_slice, ValidationError};

fn ip(s: &str) -> IpAddr {
    s.parse().expect("test ip literal parses")
}

fn base_service() -> Service {
    Service {
        namespace: "ns1".into(),
        name: "web".into(),
        cluster_ip: Some(ip("10.96.0.1")),
        service_type: ServiceType::ClusterIp,
        ports: vec![ServicePort {
            name: "http".into(),
            protocol: Protocol::Tcp,
            port: 80,
            target_port: 8080,
            node_port: None,
        }],
        session_affinity: SessionAffinity::None,
        external_traffic_policy: ExternalTrafficPolicy::Cluster,
        load_balancer_ips: vec![],
    }
}

fn base_slice() -> EndpointSlice {
    EndpointSlice {
        namespace: "ns1".into(),
        service_name: "web".into(),
        slice_name: "web-aaa".into(),
        ports: vec![EndpointPort { name: "http".into(), protocol: Protocol::Tcp, port: 8080 }],
        endpoints: vec![Endpoint {
            addresses: vec![ip("10.1.0.1")],
            conditions: EndpointConditions::default(),
            node_name: Some("node-a".into()),
            zone: Some("z1".into()),
            hints_for_zones: vec![],
        }],
    }
}

#[test]
fn valid_clusterip_service_passes() {
    assert!(validate_service(&base_service()).is_ok());
}

#[test]
fn empty_namespace_rejected() {
    let mut s = base_service();
    s.namespace = String::new();
    assert_eq!(validate_service(&s), Err(ValidationError::EmptyServiceIdentity));
}

#[test]
fn empty_name_rejected() {
    let mut s = base_service();
    s.name = String::new();
    assert_eq!(validate_service(&s), Err(ValidationError::EmptyServiceIdentity));
}

#[test]
fn clusterip_with_no_ports_rejected() {
    let mut s = base_service();
    s.ports.clear();
    assert!(matches!(validate_service(&s), Err(ValidationError::NoPorts { .. })));
}

#[test]
fn zero_service_port_rejected() {
    let mut s = base_service();
    s.ports[0].port = 0;
    assert!(matches!(validate_service(&s), Err(ValidationError::InvalidServicePort { .. })));
}

#[test]
fn zero_target_port_rejected() {
    let mut s = base_service();
    s.ports[0].target_port = 0;
    assert!(matches!(validate_service(&s), Err(ValidationError::InvalidTargetPort { .. })));
}

#[test]
fn nodeport_service_without_nodeport_rejected() {
    let mut s = base_service();
    s.service_type = ServiceType::NodePort;
    s.ports[0].node_port = None;
    assert!(matches!(validate_service(&s), Err(ValidationError::MissingNodePort { .. })));
}

#[test]
fn nodeport_service_with_nodeport_passes() {
    let mut s = base_service();
    s.service_type = ServiceType::NodePort;
    s.ports[0].node_port = Some(30080);
    assert!(validate_service(&s).is_ok());
}

#[test]
fn duplicate_port_names_rejected() {
    let mut s = base_service();
    s.ports.push(ServicePort {
        name: "http".into(),
        protocol: Protocol::Tcp,
        port: 81,
        target_port: 8081,
        node_port: None,
    });
    assert!(matches!(validate_service(&s), Err(ValidationError::DuplicatePortName { .. })));
}

#[test]
fn loadbalancer_without_ingress_rejected() {
    let mut s = base_service();
    s.service_type = ServiceType::LoadBalancer;
    s.ports[0].node_port = Some(30080);
    s.load_balancer_ips.clear();
    assert!(matches!(
        validate_service(&s),
        Err(ValidationError::LoadBalancerWithoutIngress { .. })
    ));
}

#[test]
fn externalname_service_skips_port_checks() {
    // ExternalName carries no ports/clusterIP and must still validate.
    let mut s = base_service();
    s.service_type = ServiceType::ExternalName;
    s.ports.clear();
    s.cluster_ip = None;
    assert!(validate_service(&s).is_ok());
}

#[test]
fn slice_with_empty_service_name_rejected() {
    let mut sl = base_slice();
    sl.service_name = String::new();
    assert!(matches!(validate_slice(&sl), Err(ValidationError::SliceMissingService { .. })));
}

#[test]
fn slice_endpoint_without_address_rejected() {
    let mut sl = base_slice();
    sl.endpoints[0].addresses.clear();
    assert!(matches!(validate_slice(&sl), Err(ValidationError::EndpointWithoutAddress { .. })));
}

#[test]
fn slice_zero_port_rejected() {
    let mut sl = base_slice();
    sl.ports[0].port = 0;
    assert!(matches!(validate_slice(&sl), Err(ValidationError::InvalidEndpointPort { .. })));
}

#[test]
fn well_formed_slice_passes() {
    assert!(validate_slice(&base_slice()).is_ok());
}
