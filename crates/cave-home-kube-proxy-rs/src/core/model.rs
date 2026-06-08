// SPDX-License-Identifier: Apache-2.0
//! Backend-agnostic service-proxy model.
//!
//! These types describe Services + their backing endpoints richly enough to
//! drive the proxy *decision core* (endpoint selection + rule generation)
//! independently of any wire backend (iptables / ipvs / nftables). They follow
//! the *documented* Kubernetes Service / EndpointSlice API shapes
//! (`k8s.io/api/core/v1`, `k8s.io/api/discovery/v1`) — see the crate-level
//! `port_method` note: this is a behavioural reimplementation of the public
//! Kubernetes/kube-proxy data-model and algorithm, not a verbatim source port.
//!
//! The narrower `crate::api` types are the iptables-MVP subset; this module is
//! the fuller decision-core model and is deliberately kept separate so the two
//! layers evolve independently.

use std::net::IpAddr;

/// Wire protocol of a port (`k8s.io/api/core/v1.Protocol`).
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum Protocol {
    Tcp,
    Udp,
    Sctp,
}

impl Protocol {
    /// Lower-cased canonical token (`"tcp"`, `"udp"`, `"sctp"`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
            Self::Sctp => "sctp",
        }
    }
}

/// `k8s.io/api/core/v1.ServiceType` — how a Service is exposed.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum ServiceType {
    /// In-cluster virtual IP only.
    ClusterIp,
    /// ClusterIP + a port on every node.
    NodePort,
    /// NodePort + an external load-balancer ingress IP.
    LoadBalancer,
    /// CNAME alias; never proxied (no cluster IP, no endpoints programmed).
    ExternalName,
}

/// `k8s.io/api/core/v1.ServiceAffinity` — connection stickiness.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Default)]
pub enum SessionAffinity {
    /// Each connection load-balanced independently.
    #[default]
    None,
    /// Connections from the same client IP pinned to one endpoint for a TTL.
    ClientIp {
        /// Sticky timeout in seconds (`sessionAffinityConfig.clientIP.timeoutSeconds`).
        timeout_seconds: u32,
    },
}

/// `k8s.io/api/core/v1.ServiceExternalTrafficPolicy`.
///
/// Governs which endpoints receive *external* (NodePort / LoadBalancer)
/// traffic — `Cluster` spreads to all ready endpoints (extra hop, source IP
/// rewritten); `Local` keeps traffic on the receiving node (preserves source
/// IP, no second hop) and drops external traffic on nodes with no local
/// endpoint.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Default)]
pub enum ExternalTrafficPolicy {
    #[default]
    Cluster,
    Local,
}

/// `k8s.io/api/core/v1.ServicePort`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ServicePort {
    /// Port name (unique within the Service when it has >1 port). May be empty
    /// for single-port Services.
    pub name: String,
    pub protocol: Protocol,
    /// The Service's virtual port (`port`).
    pub port: u16,
    /// The backing pod port this maps to (`targetPort`). When the slice exposes
    /// a named port this is resolved by the EndpointSlice port entry instead;
    /// numeric `target_port` is the fallback.
    pub target_port: u16,
    /// External node port (`nodePort`), only meaningful for NodePort /
    /// LoadBalancer. `None` for ClusterIP.
    pub node_port: Option<u16>,
}

/// `k8s.io/api/core/v1.Service` — the proxy-relevant subset.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Service {
    pub namespace: String,
    pub name: String,
    /// Parsed cluster VIP. `None` models a headless (`clusterIP: None`) Service.
    pub cluster_ip: Option<IpAddr>,
    pub service_type: ServiceType,
    pub ports: Vec<ServicePort>,
    pub session_affinity: SessionAffinity,
    pub external_traffic_policy: ExternalTrafficPolicy,
    /// LoadBalancer ingress IPs (`status.loadBalancer.ingress[*].ip`).
    pub load_balancer_ips: Vec<IpAddr>,
}

impl Service {
    /// `namespace/name` identity key.
    #[must_use]
    pub fn key(&self) -> String {
        format!("{}/{}", self.namespace, self.name)
    }

    /// `pkg/proxy/util.ShouldSkipService` semantics: headless (no cluster IP)
    /// and ExternalName Services are never programmed into proxy rules.
    #[must_use]
    pub const fn should_skip(&self) -> bool {
        matches!(self.service_type, ServiceType::ExternalName) || self.cluster_ip.is_none()
    }
}

/// `k8s.io/api/discovery/v1.EndpointConditions`.
///
/// All three are tri-state in the API (`*bool`); unset `ready` is treated as
/// ready, unset `serving` follows `ready`, unset `terminating` is `false`.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
pub struct EndpointConditions {
    pub ready: Option<bool>,
    pub serving: Option<bool>,
    pub terminating: Option<bool>,
}

impl EndpointConditions {
    /// `ready` defaults to `true` when unset (matches `endpointslicecache.go`).
    #[must_use]
    pub fn ready(&self) -> bool {
        self.ready.unwrap_or(true)
    }

    /// `serving` defaults to the value of `ready` when unset.
    #[must_use]
    pub fn serving(&self) -> bool {
        self.serving.unwrap_or_else(|| self.ready())
    }

    /// `terminating` defaults to `false` when unset.
    #[must_use]
    pub fn terminating(&self) -> bool {
        self.terminating.unwrap_or(false)
    }
}

/// `k8s.io/api/discovery/v1.Endpoint`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Endpoint {
    /// The first address is the canonical one (the API guarantees >=1).
    pub addresses: Vec<IpAddr>,
    pub conditions: EndpointConditions,
    /// `nodeName` hint — used by `externalTrafficPolicy: Local`.
    pub node_name: Option<String>,
    /// `zone` hint — used for topology-aware routing.
    pub zone: Option<String>,
    /// `hints.forZones[*].name` — the zones this endpoint *should* receive
    /// traffic from when topology hints are active. Empty == no hint published.
    pub hints_for_zones: Vec<String>,
}

impl Endpoint {
    /// True iff this endpoint should receive *normal* (cluster) traffic:
    /// ready, serving, and not terminating.
    #[must_use]
    pub fn is_ready_serving(&self) -> bool {
        self.conditions.ready() && self.conditions.serving() && !self.conditions.terminating()
    }

    /// Terminating-but-still-serving endpoints are used only as a fallback when
    /// a Service would otherwise have zero usable endpoints
    /// (`ProxyTerminatingEndpoints`).
    #[must_use]
    pub fn is_serving_terminating(&self) -> bool {
        self.conditions.serving() && self.conditions.terminating()
    }
}

/// `k8s.io/api/discovery/v1.EndpointPort`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EndpointPort {
    /// Matches the Service port `name` it backs. Empty for single-port.
    pub name: String,
    pub protocol: Protocol,
    pub port: u16,
}

/// `k8s.io/api/discovery/v1.EndpointSlice`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EndpointSlice {
    pub namespace: String,
    /// Value of the `kubernetes.io/service-name` label.
    pub service_name: String,
    /// Slice object name (slices are sharded; many per Service).
    pub slice_name: String,
    pub ports: Vec<EndpointPort>,
    pub endpoints: Vec<Endpoint>,
}

impl EndpointSlice {
    /// `namespace/service-name` — the parent Service key.
    #[must_use]
    pub fn service_key(&self) -> String {
        format!("{}/{}", self.namespace, self.service_name)
    }
}
