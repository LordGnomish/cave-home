// SPDX-License-Identifier: Apache-2.0
//! Hand-ported subset of `k8s.io/api/core/v1` + `k8s.io/api/discovery/v1`.
//! Only the fields needed by Phase 1 ClusterIP iptables proxying are modelled.
//! Larger surface (LoadBalancer, NodePort, IPv6, topology hints) is
//! deliberately unmapped — see `parity.manifest.toml`.

use std::fmt;

/// Upstream: `k8s.io/apimachinery/pkg/types.NamespacedName`.
#[derive(Debug, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct NamespacedName {
    pub namespace: String,
    pub name: String,
}

impl NamespacedName {
    #[must_use]
    pub fn new(namespace: impl Into<String>, name: impl Into<String>) -> Self {
        Self { namespace: namespace.into(), name: name.into() }
    }
}

impl fmt::Display for NamespacedName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Upstream NamespacedName.String() formats as "namespace/name".
        write!(f, "{}/{}", self.namespace, self.name)
    }
}

/// Upstream: `k8s.io/api/core/v1.Protocol` (`TCP`, `UDP`, `SCTP`).
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum Protocol {
    Tcp,
    Udp,
    Sctp,
}

impl Protocol {
    /// Lower-cased string used in iptables match (`-m tcp -p tcp`) and
    /// in the `portProtoHash` upstream input (`strings.ToLower`).
    #[must_use]
    pub const fn lowercase(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
            Self::Sctp => "sctp",
        }
    }
}

/// Upstream: `pkg/proxy/types.go ServicePortName`.
#[derive(Debug, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ServicePortName {
    pub namespaced_name: NamespacedName,
    pub port: String,
    pub protocol: Protocol,
}

impl fmt::Display for ServicePortName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Upstream: fmt.Sprintf("%s%s", nn.String(), fmtPortName(port))
        // where fmtPortName returns ":<port>" or "" if empty.
        if self.port.is_empty() {
            write!(f, "{}", self.namespaced_name)
        } else {
            write!(f, "{}:{}", self.namespaced_name, self.port)
        }
    }
}

/// Upstream: `k8s.io/api/core/v1.ServiceType` (subset).
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ServiceType {
    /// `ClusterIP` — only mode actively proxied in Phase 1.
    ClusterIP,
    /// `NodePort` — Phase 1b.
    NodePort,
    /// `LoadBalancer` — Phase 1b.
    LoadBalancer,
    /// `ExternalName` — never proxied (skip).
    ExternalName,
}

/// Upstream: `k8s.io/api/core/v1.ServicePort` (subset).
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ServicePort {
    pub name: String,
    pub port: i32,
    pub protocol: Protocol,
}

/// Upstream: `k8s.io/api/core/v1.Service` (subset — Phase 1 ClusterIP only).
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Service {
    pub metadata: NamespacedName,
    pub cluster_ip: String,
    pub ports: Vec<ServicePort>,
    pub type_: ServiceType,
}

impl Service {
    /// Upstream: `pkg/proxy/util/utils.go ShouldSkipService`.
    #[must_use]
    pub fn should_skip(&self) -> bool {
        // ClusterIP None or empty → skip.
        if self.cluster_ip.is_empty() || self.cluster_ip == "None" {
            return true;
        }
        // ExternalName services are never proxied.
        matches!(self.type_, ServiceType::ExternalName)
    }
}

/// Upstream: `k8s.io/api/discovery/v1.EndpointConditions`.
#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct EndpointConditions {
    pub ready: Option<bool>,
    pub serving: Option<bool>,
    pub terminating: Option<bool>,
}

/// Upstream: `k8s.io/api/discovery/v1.Endpoint`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Endpoint {
    pub addresses: Vec<String>,
    pub conditions: EndpointConditions,
}

impl Endpoint {
    /// Upstream: an endpoint is "ready" when `Conditions.Ready` is nil OR true
    /// (see `pkg/proxy/endpointslicecache.go addEndpoints`).
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        match self.conditions.ready {
            None => true,
            Some(r) => r,
        }
    }
}

/// Upstream: `k8s.io/api/discovery/v1.EndpointPort`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EndpointPort {
    pub name: String,
    pub port: i32,
    pub protocol: Protocol,
}

/// Upstream: `k8s.io/api/discovery/v1.EndpointSlice`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EndpointSlice {
    pub metadata: NamespacedName,
    /// Value of the `kubernetes.io/service-name` label.
    pub service_name: String,
    pub ports: Vec<EndpointPort>,
    pub endpoints: Vec<Endpoint>,
}

impl EndpointSlice {
    /// Returns the `NamespacedName` of the parent Service (slice namespace +
    /// `kubernetes.io/service-name` label value).
    #[must_use]
    pub fn service_namespaced_name(&self) -> NamespacedName {
        NamespacedName::new(self.metadata.namespace.clone(), self.service_name.clone())
    }
}

/// Upstream: `k8s.io/apimachinery/pkg/watch.Event` (subset — only the variants
/// the proxier reacts to). `Bookmark` is informational; `Error` is propagated
/// by the EventSource trait via `Result`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum WatchEvent {
    ServiceAdded(Service),
    ServiceModified(Service),
    ServiceDeleted(Service),
    EndpointSliceAdded(EndpointSlice),
    EndpointSliceModified(EndpointSlice),
    EndpointSliceDeleted(EndpointSlice),
}
