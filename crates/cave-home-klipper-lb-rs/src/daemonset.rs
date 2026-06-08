// SPDX-License-Identifier: Apache-2.0
//! svclb `DaemonSet` pod-spec construction.
//!
//! For each LoadBalancer Service, K3s's ServiceLB controller creates a
//! `DaemonSet` named `svclb-<service>`; each pod has one container per Service
//! port. Each container runs the `klipper-lb` image and is parameterised purely
//! through environment variables that `entry.sh` reads:
//!
//! * `SRC_PORT`   — the host port the container binds (the Service `port`),
//! * `SRC_RANGE`  — the source CIDR allowed to reach it (`0.0.0.0/0` default),
//! * `DEST_PROTO` — `TCP` / `UDP`,
//! * `DEST_PORT`  — the cluster Service port to forward to (the NodePort),
//! * `DEST_IPS`   — the destination IP(s) the traffic is forwarded to.
//!
//! This module builds that spec as plain data so it can be asserted in tests.
//! It does **not** apply the DaemonSet to a cluster — that (and the in-pod
//! iptables the container programs from these vars) is deferred Phase 1b.

use crate::service::{LoadBalancerService, Protocol, ServicePort};

/// One environment variable on a svclb container.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EnvVar {
    pub name: String,
    pub value: String,
}

impl EnvVar {
    fn new(name: &str, value: impl Into<String>) -> Self {
        Self {
            name: name.to_owned(),
            value: value.into(),
        }
    }
}

/// One container of the svclb pod — one per Service port.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SvclbContainer {
    /// Container name K3s derives: `lb-<proto>-<port>` (lower-cased proto).
    pub name: String,
    /// The host port this container binds (`hostPort` == Service `port`).
    pub host_port: u16,
    pub protocol: Protocol,
    /// The env vars `entry.sh` consumes.
    pub env: Vec<EnvVar>,
}

/// The svclb pod spec for one Service: its `DaemonSet` name + per-port
/// containers.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SvclbPodSpec {
    /// `svclb-<service>`.
    pub daemonset_name: String,
    /// `namespace/name` of the source Service.
    pub service_key: String,
    pub containers: Vec<SvclbContainer>,
}

/// The default source range klipper-lb uses when the Service sets no
/// `loadBalancerSourceRanges`.
const DEFAULT_SRC_RANGE: &str = "0.0.0.0/0";

/// Build the env block for one container.
///
/// `dest_ips` is the comma-joined destination set the container forwards to
/// (the cluster Service / NodePort target). For svclb this is conventionally
/// the cluster's Service ClusterIP or NodePort target; the controller resolves
/// it. We accept it as a parameter so the spec is self-contained and testable.
fn container_env(port: &ServicePort, dest_ips: &str) -> Vec<EnvVar> {
    vec![
        EnvVar::new("SRC_PORT", port.port.to_string()),
        EnvVar::new("SRC_RANGE", DEFAULT_SRC_RANGE),
        EnvVar::new("DEST_PROTO", port.protocol.as_str()),
        EnvVar::new("DEST_PORT", port.node_port.to_string()),
        EnvVar::new("DEST_IPS", dest_ips.to_owned()),
    ]
}

/// Build one container for a Service port.
fn build_container(port: &ServicePort, dest_ips: &str) -> SvclbContainer {
    SvclbContainer {
        name: format!(
            "lb-{}-{}",
            port.protocol.as_str().to_ascii_lowercase(),
            port.port
        ),
        host_port: port.port,
        protocol: port.protocol,
        env: container_env(port, dest_ips),
    }
}

/// Build the full svclb pod spec for a Service.
///
/// `dest_ips` are the destination IP(s) all containers forward to (typically
/// the cluster Service ClusterIP); they are comma-joined into `DEST_IPS` as
/// klipper-lb expects. An empty `dest_ips` yields an empty `DEST_IPS` value —
/// callers should validate the Service first.
#[must_use]
pub fn build_pod_spec(svc: &LoadBalancerService, dest_ips: &[std::net::IpAddr]) -> SvclbPodSpec {
    let joined = dest_ips
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    SvclbPodSpec {
        daemonset_name: svc.svclb_daemonset_name(),
        service_key: svc.key(),
        containers: svc
            .ports
            .iter()
            .map(|p| build_container(p, &joined))
            .collect(),
    }
}

impl SvclbContainer {
    /// Look up an env var value by name.
    #[must_use]
    pub fn env_value(&self, name: &str) -> Option<&str> {
        self.env
            .iter()
            .find(|e| e.name == name)
            .map(|e| e.value.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::ExternalTrafficPolicy;
    use std::collections::BTreeMap;
    use std::net::IpAddr;

    fn ip(s: &str) -> IpAddr {
        s.parse().expect("test ip")
    }

    fn svc(name: &str, ports: Vec<ServicePort>) -> LoadBalancerService {
        LoadBalancerService {
            namespace: "default".to_owned(),
            name: name.to_owned(),
            load_balancer_ips: vec![],
            ports,
            external_traffic_policy: ExternalTrafficPolicy::Cluster,
            node_selector: BTreeMap::new(),
        }
    }

    fn port(name: &str, proto: Protocol, p: u16, np: u16) -> ServicePort {
        ServicePort {
            name: name.to_owned(),
            protocol: proto,
            port: p,
            node_port: np,
        }
    }

    #[test]
    fn daemonset_name_is_svclb_prefixed() {
        let spec = build_pod_spec(
            &svc("web", vec![port("http", Protocol::Tcp, 80, 30080)]),
            &[ip("10.43.0.10")],
        );
        assert_eq!(spec.daemonset_name, "svclb-web");
        assert_eq!(spec.service_key, "default/web");
    }

    #[test]
    fn one_container_per_port() {
        let spec = build_pod_spec(
            &svc(
                "web",
                vec![
                    port("http", Protocol::Tcp, 80, 30080),
                    port("https", Protocol::Tcp, 443, 30443),
                ],
            ),
            &[ip("10.43.0.10")],
        );
        assert_eq!(spec.containers.len(), 2);
    }

    #[test]
    fn container_name_encodes_proto_and_port() {
        let spec = build_pod_spec(
            &svc("dns", vec![port("dns", Protocol::Udp, 53, 30053)]),
            &[ip("10.43.0.10")],
        );
        assert_eq!(spec.containers[0].name, "lb-udp-53");
    }

    #[test]
    fn env_vars_match_klipper_lb_contract() {
        let spec = build_pod_spec(
            &svc("web", vec![port("http", Protocol::Tcp, 80, 30080)]),
            &[ip("10.43.0.10")],
        );
        let c = &spec.containers[0];
        assert_eq!(c.host_port, 80);
        assert_eq!(c.env_value("SRC_PORT"), Some("80"));
        assert_eq!(c.env_value("SRC_RANGE"), Some("0.0.0.0/0"));
        assert_eq!(c.env_value("DEST_PROTO"), Some("TCP"));
        assert_eq!(c.env_value("DEST_PORT"), Some("30080"));
        assert_eq!(c.env_value("DEST_IPS"), Some("10.43.0.10"));
    }

    #[test]
    fn udp_proto_is_uppercased_in_env() {
        let spec = build_pod_spec(
            &svc("dns", vec![port("dns", Protocol::Udp, 53, 30053)]),
            &[ip("10.43.0.10")],
        );
        assert_eq!(spec.containers[0].env_value("DEST_PROTO"), Some("UDP"));
    }

    #[test]
    fn dest_ips_are_comma_joined_for_dual_stack() {
        let spec = build_pod_spec(
            &svc("web", vec![port("http", Protocol::Tcp, 80, 30080)]),
            &[ip("10.43.0.10"), ip("fd00::10")],
        );
        assert_eq!(
            spec.containers[0].env_value("DEST_IPS"),
            Some("10.43.0.10,fd00::10")
        );
    }

    #[test]
    fn every_container_has_the_five_env_vars() {
        let spec = build_pod_spec(
            &svc(
                "web",
                vec![
                    port("http", Protocol::Tcp, 80, 30080),
                    port("dns", Protocol::Udp, 53, 30053),
                ],
            ),
            &[ip("10.43.0.10")],
        );
        for c in &spec.containers {
            assert_eq!(c.env.len(), 5);
            for name in ["SRC_PORT", "SRC_RANGE", "DEST_PROTO", "DEST_PORT", "DEST_IPS"] {
                assert!(c.env_value(name).is_some(), "missing {name}");
            }
        }
    }

    #[test]
    fn multi_port_dest_ports_track_their_node_ports() {
        let spec = build_pod_spec(
            &svc(
                "web",
                vec![
                    port("http", Protocol::Tcp, 80, 30080),
                    port("https", Protocol::Tcp, 443, 30443),
                ],
            ),
            &[ip("10.43.0.10")],
        );
        assert_eq!(spec.containers[0].env_value("DEST_PORT"), Some("30080"));
        assert_eq!(spec.containers[1].env_value("DEST_PORT"), Some("30443"));
    }
}
