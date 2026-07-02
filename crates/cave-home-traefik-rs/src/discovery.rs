// SPDX-License-Identifier: Apache-2.0
//! Kubernetes service discovery: resolving a `Service` backend to the concrete
//! [`Server`] pool the load balancer fans across.
//!
//! Spec basis: Traefik's `kubernetes-ingress` provider resolves an Ingress
//! backend (service name + port) to endpoints. With the default
//! `nativeLBByDefault=false` behaviour it load-balances across the ready pod
//! endpoints (`EndpointSlice`); the `ClusterIP` mode instead targets the
//! service's virtual IP. Both are pure mappings here — the watch/informer that
//! produces these objects is the listener's job.

use crate::loadbalancer::Server;

/// A reference to a service port: numeric, or a named port resolved via the
/// endpoint slice's port table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortRef {
    /// A numeric port.
    Number(u16),
    /// A named port (matched against `EndpointPort.name`).
    Name(String),
}

/// A port entry within an `EndpointSlice`.
#[derive(Debug, Clone)]
pub struct EndpointPort {
    /// The port name, if any.
    pub name: Option<String>,
    /// The port number.
    pub port: u16,
}

/// A single endpoint (a pod) within an `EndpointSlice`.
#[derive(Debug, Clone)]
pub struct Endpoint {
    /// The endpoint's addresses (usually one pod IP).
    pub addresses: Vec<String>,
    /// Whether the endpoint passes its readiness condition.
    pub ready: bool,
}

/// A Kubernetes `EndpointSlice`.
#[derive(Debug, Clone, Default)]
pub struct EndpointSlice {
    /// The slice's port table.
    pub ports: Vec<EndpointPort>,
    /// The slice's endpoints.
    pub endpoints: Vec<Endpoint>,
}

/// Resolve a [`PortRef`] to a concrete port number using the slices' port
/// tables (numeric refs pass through).
#[must_use]
pub fn resolve_port(slices: &[EndpointSlice], port: &PortRef) -> Option<u16> {
    match port {
        PortRef::Number(n) => Some(*n),
        PortRef::Name(name) => slices
            .iter()
            .flat_map(|s| &s.ports)
            .find(|p| p.name.as_deref() == Some(name.as_str()))
            .map(|p| p.port),
    }
}

/// Format `host:port` as a dial authority, bracketing IPv6 literals.
fn authority(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

/// Build the [`Server`] pool from ready endpoint addresses for `port`, dialing
/// each over `scheme` (`http`/`https`). Not-ready endpoints are skipped.
#[must_use]
pub fn endpoints_to_servers(slices: &[EndpointSlice], port: &PortRef, scheme: &str) -> Vec<Server> {
    let Some(port) = resolve_port(slices, port) else {
        return Vec::new();
    };
    let mut servers = Vec::new();
    for slice in slices {
        for ep in &slice.endpoints {
            if !ep.ready {
                continue;
            }
            for addr in &ep.addresses {
                servers.push(Server::new(&format!("{scheme}://{}", authority(addr, port))));
            }
        }
    }
    servers
}

/// Build a single-server pool targeting a service's `ClusterIP`.
#[must_use]
pub fn cluster_ip_servers(cluster_ip: &str, port: u16, scheme: &str) -> Vec<Server> {
    vec![Server::new(&format!("{scheme}://{}", authority(cluster_ip, port)))]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slice() -> EndpointSlice {
        EndpointSlice {
            ports: vec![EndpointPort { name: Some("web".to_string()), port: 8080 }],
            endpoints: vec![
                Endpoint { addresses: vec!["10.0.0.1".to_string()], ready: true },
                Endpoint { addresses: vec!["10.0.0.2".to_string()], ready: true },
                Endpoint { addresses: vec!["10.0.0.3".to_string()], ready: false },
            ],
        }
    }

    #[test]
    fn numeric_port_passes_through() {
        assert_eq!(resolve_port(&[slice()], &PortRef::Number(9000)), Some(9000));
    }

    #[test]
    fn named_port_is_resolved() {
        assert_eq!(resolve_port(&[slice()], &PortRef::Name("web".to_string())), Some(8080));
        assert_eq!(resolve_port(&[slice()], &PortRef::Name("missing".to_string())), None);
    }

    #[test]
    fn ready_endpoints_become_servers() {
        let servers = endpoints_to_servers(&[slice()], &PortRef::Name("web".to_string()), "http");
        let urls: Vec<&str> = servers.iter().map(|s| s.url.as_str()).collect();
        assert_eq!(urls, vec!["http://10.0.0.1:8080", "http://10.0.0.2:8080"]);
    }

    #[test]
    fn ipv6_addresses_are_bracketed() {
        let s = EndpointSlice {
            ports: vec![EndpointPort { name: None, port: 443 }],
            endpoints: vec![Endpoint { addresses: vec!["fd00::1".to_string()], ready: true }],
        };
        let servers = endpoints_to_servers(&[s], &PortRef::Number(443), "https");
        assert_eq!(servers[0].url, "https://[fd00::1]:443");
    }

    #[test]
    fn unresolvable_port_yields_no_servers() {
        let servers = endpoints_to_servers(&[slice()], &PortRef::Name("nope".to_string()), "http");
        assert!(servers.is_empty());
    }

    #[test]
    fn cluster_ip_is_single_server() {
        let servers = cluster_ip_servers("10.96.0.10", 80, "http");
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].url, "http://10.96.0.10:80");
    }
}
