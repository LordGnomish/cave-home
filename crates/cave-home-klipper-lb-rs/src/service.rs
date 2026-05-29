// SPDX-License-Identifier: Apache-2.0
//! LoadBalancer-type Service model + validation.
//!
//! These types describe the `LoadBalancer` Services that klipper-lb / svclb is
//! responsible for. They follow the documented Kubernetes
//! `k8s.io/api/core/v1.Service` shape — the proxy-relevant subset — and are a
//! behavioural reimplementation, not a verbatim source port.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::net::IpAddr;

/// Wire protocol of a Service port (`k8s.io/api/core/v1.Protocol`).
///
/// klipper-lb's svclb pod only forwards TCP and UDP; SCTP exists in the API but
/// is rejected here because the svclb forwarder (`klipper-lb` `entry.sh`) has no
/// SCTP path.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum Protocol {
    Tcp,
    Udp,
}

impl Protocol {
    /// Upper-cased token as it appears in the Service spec / svclb env
    /// (`DEST_PROTO`): `"TCP"` / `"UDP"`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tcp => "TCP",
            Self::Udp => "UDP",
        }
    }

    /// Parse a protocol token case-insensitively. SCTP and unknown tokens are
    /// rejected (`None`).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "TCP" => Some(Self::Tcp),
            "UDP" => Some(Self::Udp),
            _ => None,
        }
    }
}

/// `k8s.io/api/core/v1.ServiceExternalTrafficPolicy`.
///
/// `Cluster` publishes the Service on every eligible node; `Local` publishes it
/// only on nodes that run a ready backing pod (preserving source IP and
/// avoiding a second hop).
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Default)]
pub enum ExternalTrafficPolicy {
    #[default]
    Cluster,
    Local,
}

/// One port of a LoadBalancer Service (`k8s.io/api/core/v1.ServicePort`).
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ServicePort {
    /// Port name (unique within a multi-port Service; may be empty for single
    /// port).
    pub name: String,
    pub protocol: Protocol,
    /// The Service's published port (`port`) — svclb binds this on the host.
    pub port: u16,
    /// Allocated NodePort (`nodePort`). klipper-lb forwards the host port to the
    /// cluster Service; the NodePort is the in-cluster handle it targets.
    pub node_port: u16,
}

/// A LoadBalancer-type Service that klipper-lb is asked to expose
/// (`k8s.io/api/core/v1.Service` with `spec.type: LoadBalancer`).
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LoadBalancerService {
    pub namespace: String,
    pub name: String,
    /// Requested ingress IPs (`spec.loadBalancerIP` / the dual-stack
    /// `loadBalancerIPs`). Empty means "publish the node IPs".
    pub load_balancer_ips: Vec<IpAddr>,
    pub ports: Vec<ServicePort>,
    pub external_traffic_policy: ExternalTrafficPolicy,
    /// `metadata.labels` placed on the svclb pod's node affinity — restricts
    /// which nodes run the svclb pod. Empty == all eligible nodes.
    pub node_selector: BTreeMap<String, String>,
}

impl LoadBalancerService {
    /// `namespace/name` identity key.
    #[must_use]
    pub fn key(&self) -> String {
        format!("{}/{}", self.namespace, self.name)
    }

    /// The svclb `DaemonSet` name K3s derives for this Service: `svclb-<name>`.
    #[must_use]
    pub fn svclb_daemonset_name(&self) -> String {
        format!("svclb-{}", self.name)
    }
}

/// Why a [`LoadBalancerService`] was rejected before it could be programmed.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ServiceError {
    /// Namespace or name was empty.
    EmptyIdentity,
    /// The Service declared no ports.
    NoPorts { key: String },
    /// A `port` was zero (invalid).
    InvalidPort { key: String, name: String },
    /// A `nodePort` was zero (invalid for a LoadBalancer Service).
    InvalidNodePort { key: String, name: String },
    /// Two ports share a (protocol, port) pair within one Service.
    DuplicatePort {
        key: String,
        protocol: Protocol,
        port: u16,
    },
    /// Two named ports share a name.
    DuplicatePortName { key: String, name: String },
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyIdentity => write!(f, "service namespace/name must be non-empty"),
            Self::NoPorts { key } => write!(f, "service {key} declares no ports"),
            Self::InvalidPort { key, name } => {
                write!(f, "service {key} port {name:?} has invalid port 0")
            }
            Self::InvalidNodePort { key, name } => {
                write!(f, "service {key} port {name:?} has invalid nodePort 0")
            }
            Self::DuplicatePort {
                key,
                protocol,
                port,
            } => write!(
                f,
                "service {key} declares {proto}/{port} twice",
                proto = protocol.as_str()
            ),
            Self::DuplicatePortName { key, name } => {
                write!(f, "service {key} port name {name:?} is not unique")
            }
        }
    }
}

impl Error for ServiceError {}

/// Validate a [`LoadBalancerService`] structurally before programming.
///
/// # Errors
/// Returns the first [`ServiceError`] encountered. Mirrors the documented
/// Service API invariants relevant to svclb; tolerant input handling (log +
/// skip) is the controller's job, so this never panics.
pub fn validate_service(svc: &LoadBalancerService) -> Result<(), ServiceError> {
    if svc.namespace.is_empty() || svc.name.is_empty() {
        return Err(ServiceError::EmptyIdentity);
    }
    let key = svc.key();
    if svc.ports.is_empty() {
        return Err(ServiceError::NoPorts { key });
    }

    let mut seen_names = std::collections::BTreeSet::new();
    let mut seen_ports = std::collections::BTreeSet::new();
    for p in &svc.ports {
        if p.port == 0 {
            return Err(ServiceError::InvalidPort {
                key,
                name: p.name.clone(),
            });
        }
        if p.node_port == 0 {
            return Err(ServiceError::InvalidNodePort {
                key,
                name: p.name.clone(),
            });
        }
        if !p.name.is_empty() && !seen_names.insert(p.name.clone()) {
            return Err(ServiceError::DuplicatePortName {
                key,
                name: p.name.clone(),
            });
        }
        if !seen_ports.insert((p.protocol, p.port)) {
            return Err(ServiceError::DuplicatePort {
                key,
                protocol: p.protocol,
                port: p.port,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(name: &str, proto: Protocol, port: u16, np: u16) -> ServicePort {
        ServicePort {
            name: name.to_owned(),
            protocol: proto,
            port,
            node_port: np,
        }
    }

    fn svc(ports: Vec<ServicePort>) -> LoadBalancerService {
        LoadBalancerService {
            namespace: "default".to_owned(),
            name: "web".to_owned(),
            load_balancer_ips: vec![],
            ports,
            external_traffic_policy: ExternalTrafficPolicy::Cluster,
            node_selector: BTreeMap::new(),
        }
    }

    #[test]
    fn protocol_parse_is_case_insensitive_and_rejects_sctp() {
        assert_eq!(Protocol::parse("tcp"), Some(Protocol::Tcp));
        assert_eq!(Protocol::parse("UDP"), Some(Protocol::Udp));
        assert_eq!(Protocol::parse("Tcp"), Some(Protocol::Tcp));
        assert_eq!(Protocol::parse("SCTP"), None);
        assert_eq!(Protocol::parse("garbage"), None);
    }

    #[test]
    fn protocol_as_str_is_uppercase() {
        assert_eq!(Protocol::Tcp.as_str(), "TCP");
        assert_eq!(Protocol::Udp.as_str(), "UDP");
    }

    #[test]
    fn valid_single_port_service_passes() {
        assert!(validate_service(&svc(vec![port("http", Protocol::Tcp, 80, 30080)])).is_ok());
    }

    #[test]
    fn valid_multi_port_service_passes() {
        let s = svc(vec![
            port("http", Protocol::Tcp, 80, 30080),
            port("https", Protocol::Tcp, 443, 30443),
            port("dns", Protocol::Udp, 53, 30053),
        ]);
        assert!(validate_service(&s).is_ok());
    }

    #[test]
    fn empty_identity_rejected() {
        let mut s = svc(vec![port("http", Protocol::Tcp, 80, 30080)]);
        s.name = String::new();
        assert_eq!(validate_service(&s), Err(ServiceError::EmptyIdentity));
    }

    #[test]
    fn no_ports_rejected() {
        assert!(matches!(
            validate_service(&svc(vec![])),
            Err(ServiceError::NoPorts { .. })
        ));
    }

    #[test]
    fn zero_port_rejected() {
        assert!(matches!(
            validate_service(&svc(vec![port("http", Protocol::Tcp, 0, 30080)])),
            Err(ServiceError::InvalidPort { .. })
        ));
    }

    #[test]
    fn zero_node_port_rejected() {
        assert!(matches!(
            validate_service(&svc(vec![port("http", Protocol::Tcp, 80, 0)])),
            Err(ServiceError::InvalidNodePort { .. })
        ));
    }

    #[test]
    fn duplicate_port_name_rejected() {
        let s = svc(vec![
            port("dup", Protocol::Tcp, 80, 30080),
            port("dup", Protocol::Tcp, 443, 30443),
        ]);
        assert!(matches!(
            validate_service(&s),
            Err(ServiceError::DuplicatePortName { .. })
        ));
    }

    #[test]
    fn duplicate_proto_port_pair_rejected() {
        let s = svc(vec![
            port("a", Protocol::Tcp, 80, 30080),
            port("b", Protocol::Tcp, 80, 30081),
        ]);
        assert!(matches!(
            validate_service(&s),
            Err(ServiceError::DuplicatePort { .. })
        ));
    }

    #[test]
    fn same_port_different_proto_is_allowed() {
        // TCP/53 and UDP/53 can coexist (e.g. DNS).
        let s = svc(vec![
            port("dns-tcp", Protocol::Tcp, 53, 30053),
            port("dns-udp", Protocol::Udp, 53, 30054),
        ]);
        assert!(validate_service(&s).is_ok());
    }

    #[test]
    fn key_and_daemonset_name() {
        let s = svc(vec![port("http", Protocol::Tcp, 80, 30080)]);
        assert_eq!(s.key(), "default/web");
        assert_eq!(s.svclb_daemonset_name(), "svclb-web");
    }

    #[test]
    fn error_displays_without_panic() {
        let e = ServiceError::NoPorts {
            key: "default/web".to_owned(),
        };
        assert!(e.to_string().contains("default/web"));
    }
}
