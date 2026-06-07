// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Kubernetes API integration for the `kubernetes` plugin.
//!
//! The [`crate::kubernetes`] plugin resolves against an in-memory service
//! registry; this module is what `CoreDNS` fills that registry from — the
//! `plugin/kubernetes/controller.go` indexer and `object/` converters that turn
//! API list objects into the plugin's internal records.
//!
//! Two layers, matching the crate's honest-port split:
//!
//! * **Conversion** ([`kubernetes_from_api`]) — the real logic: decode a
//!   `ServiceList` and `Endpoints` list (the JSON the Kubernetes API serves)
//!   and fold them into a populated [`Kubernetes`] plugin, reproducing
//!   `CoreDNS`'s `ToService` / `ToEndpoints` rules (`ClusterIP`, headless,
//!   `ExternalName`, named `SRV` ports).
//! * **Transport seam** ([`ApiSource`]) — the abstract connection to the API
//!   server. The live watched HTTP client is the deferred I/O shell; tests
//!   drive a [`StaticSource`] holding canned list responses.

use serde::Deserialize;
use std::collections::HashMap;
use std::net::IpAddr;

use crate::error::{Result, WireError};
use crate::kubernetes::{Endpoint, Kubernetes, Port, Service};
use crate::name::Name;

/// A Kubernetes `ServiceList` (`/api/v1/services`).
#[derive(Debug, Deserialize, Default)]
struct ServiceList {
    #[serde(default)]
    items: Vec<ServiceItem>,
}

/// One `Service` object.
#[derive(Debug, Deserialize)]
struct ServiceItem {
    metadata: Meta,
    #[serde(default)]
    spec: ServiceSpec,
}

/// The subset of `ObjectMeta` the indexer needs.
#[derive(Debug, Deserialize, Default)]
struct Meta {
    #[serde(default)]
    name: String,
    #[serde(default)]
    namespace: String,
}

/// The subset of `ServiceSpec` the indexer needs.
#[derive(Debug, Deserialize, Default)]
struct ServiceSpec {
    // Kubernetes spells these with a capital `IP`, so the camelCase default
    // does not match — name them explicitly.
    #[serde(rename = "clusterIP", default)]
    cluster_ip: Option<String>,
    #[serde(rename = "clusterIPs", default)]
    cluster_ips: Vec<String>,
    #[serde(rename = "type", default)]
    service_type: Option<String>,
    #[serde(rename = "externalName", default)]
    external_name: Option<String>,
    #[serde(default)]
    ports: Vec<ServicePort>,
}

/// A named service port.
#[derive(Debug, Deserialize, Default)]
struct ServicePort {
    #[serde(default)]
    name: String,
    #[serde(default)]
    protocol: Option<String>,
    #[serde(default)]
    port: u16,
}

/// A Kubernetes `Endpoints` list (`/api/v1/endpoints`).
#[derive(Debug, Deserialize, Default)]
struct EndpointsList {
    #[serde(default)]
    items: Vec<EndpointsItem>,
}

/// One `Endpoints` object (one per headless service).
#[derive(Debug, Deserialize)]
struct EndpointsItem {
    metadata: Meta,
    #[serde(default)]
    subsets: Vec<EndpointSubset>,
}

/// One `EndpointSubset`: a set of addresses sharing a set of ports.
#[derive(Debug, Deserialize, Default)]
struct EndpointSubset {
    #[serde(default)]
    addresses: Vec<EndpointAddress>,
}

/// One backing address behind a headless service.
#[derive(Debug, Deserialize, Default)]
struct EndpointAddress {
    #[serde(default)]
    ip: String,
    #[serde(default)]
    hostname: Option<String>,
}

/// Build a populated [`Kubernetes`] plugin for `zone` from the API's
/// `ServiceList` and `Endpoints` list JSON.
///
/// # Conversion
/// This is `CoreDNS`'s `ToService` / `ToEndpoints` conversion: a `ClusterIP`
/// service becomes its addresses, a `clusterIP: None` service is headless and
/// draws its addresses from the matching `Endpoints` object, and an
/// `ExternalName` service becomes a `CNAME` alias. Named ports become `SRV`
/// records via [`Service::with_port`].
///
/// # Errors
/// [`WireError::Config`] if either document fails to decode as JSON, or an
/// `ExternalName` target is not a valid domain name.
pub fn kubernetes_from_api(
    zone: &str,
    services_json: &str,
    endpoints_json: &str,
) -> Result<Kubernetes> {
    let services: ServiceList =
        serde_json::from_str(services_json).map_err(|_| WireError::Config {
            reason: "kubernetes: malformed ServiceList JSON",
        })?;
    let endpoints: EndpointsList =
        serde_json::from_str(endpoints_json).map_err(|_| WireError::Config {
            reason: "kubernetes: malformed Endpoints JSON",
        })?;

    // Index endpoints by (namespace, name) for the headless lookup.
    let mut by_service: HashMap<(&str, &str), Vec<Endpoint>> = HashMap::new();
    for item in &endpoints.items {
        let key = (
            item.metadata.namespace.as_str(),
            item.metadata.name.as_str(),
        );
        let bucket = by_service.entry(key).or_default();
        for subset in &item.subsets {
            for addr in &subset.addresses {
                if let Ok(ip) = addr.ip.parse::<IpAddr>() {
                    bucket.push(Endpoint::new(addr.hostname.as_deref(), ip));
                }
            }
        }
    }

    let mut k = Kubernetes::new(zone);
    for item in &services.items {
        let ns = item.metadata.namespace.as_str();
        let name = item.metadata.name.as_str();
        k = k.with_namespace(ns);

        let service = if item.spec.service_type.as_deref() == Some("ExternalName") {
            let target_str = item.spec.external_name.as_deref().unwrap_or("");
            let target = Name::parse(target_str).map_err(|_| WireError::Config {
                reason: "kubernetes: bad ExternalName target",
            })?;
            Service::external_name(ns, name, target)
        } else if item.spec.cluster_ip.as_deref() == Some("None") {
            let eps = by_service.remove(&(ns, name)).unwrap_or_default();
            Service::headless(ns, name, eps)
        } else {
            Service::cluster_ip(ns, name, cluster_addresses(&item.spec))
        };

        // Attach named ports for SRV.
        let service = item.spec.ports.iter().fold(service, |svc, p| {
            let is_udp = p
                .protocol
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case("UDP"));
            let port = if is_udp {
                Port::udp(&p.name, p.port)
            } else {
                Port::tcp(&p.name, p.port)
            };
            svc.with_port(port)
        });
        k = k.with_service(service);
    }
    Ok(k)
}

/// The cluster addresses of a normal service: `clusterIPs` if present, else the
/// singular `clusterIP`. The placeholder `None` and unparseable values are
/// skipped (a headless service is handled before this is called).
fn cluster_addresses(spec: &ServiceSpec) -> Vec<IpAddr> {
    let raw: Vec<&String> = if spec.cluster_ips.is_empty() {
        spec.cluster_ip.iter().collect()
    } else {
        spec.cluster_ips.iter().collect()
    };
    raw.into_iter()
        .filter(|s| s.as_str() != "None")
        .filter_map(|s| s.parse::<IpAddr>().ok())
        .collect()
}

/// A point-in-time copy of the cluster's `Service` and `Endpoints` lists, as
/// the API served them.
///
/// Unlike the live [`Kubernetes`] plugin (which is `!Send`), a snapshot is
/// plain `Send + Clone` data, so it can be handed to the resolver actor
/// ([`crate::server::Resolver::update_endpoints`]); the actor rebuilds its
/// chain's kubernetes plugin from it. This is how a watch update reaches the
/// running server in this crate's immutable-plugin design.
#[derive(Debug, Clone, Default)]
pub struct K8sSnapshot {
    /// The `ServiceList` JSON.
    pub services: String,
    /// The `Endpoints` list JSON.
    pub endpoints: String,
}

impl K8sSnapshot {
    /// A snapshot from the two list documents.
    #[must_use]
    pub fn new(services: &str, endpoints: &str) -> Self {
        Self {
            services: services.to_string(),
            endpoints: endpoints.to_string(),
        }
    }

    /// Convert into a populated [`Kubernetes`] plugin for `zone`.
    ///
    /// # Errors
    /// As [`kubernetes_from_api`].
    pub fn resolve(&self, zone: &str) -> Result<Kubernetes> {
        kubernetes_from_api(zone, &self.services, &self.endpoints)
    }
}

/// The abstract Kubernetes API connection: the source of the `Service` and
/// `Endpoints` lists the indexer converts.
///
/// The live watching client is the deferred I/O shell; this seam lets the
/// conversion be tested without one.
pub trait ApiSource {
    /// Fetch the current `ServiceList` JSON (`/api/v1/services`).
    fn list_services(&self) -> impl std::future::Future<Output = std::io::Result<String>> + Send;
    /// Fetch the current `Endpoints` list JSON (`/api/v1/endpoints`).
    fn list_endpoints(&self) -> impl std::future::Future<Output = std::io::Result<String>> + Send;
}

/// A canned [`ApiSource`] for tests and offline builds: it returns fixed list
/// JSON instead of talking to an API server.
#[derive(Debug, Clone, Default)]
pub struct StaticSource {
    /// The `ServiceList` JSON to return.
    pub services: String,
    /// The `Endpoints` list JSON to return.
    pub endpoints: String,
}

impl ApiSource for StaticSource {
    async fn list_services(&self) -> std::io::Result<String> {
        Ok(self.services.clone())
    }
    async fn list_endpoints(&self) -> std::io::Result<String> {
        Ok(self.endpoints.clone())
    }
}

/// Build a populated [`Kubernetes`] plugin for `zone` by fetching and converting
/// the current `Service`/`Endpoints` lists from `source`.
///
/// # Errors
/// A transport [`std::io::Error`] is surfaced as [`WireError::Config`]; a JSON
/// decode failure likewise.
// The returned future is intentionally not `Send`: it is awaited only on the
// resolver's single-thread runtime (see [`crate::server`]), so requiring the
// `ApiSource` to be `Sync` would be a needless bound on callers.
#[allow(clippy::future_not_send)]
pub async fn kubernetes_from_source(zone: &str, source: &impl ApiSource) -> Result<Kubernetes> {
    let services = source
        .list_services()
        .await
        .map_err(|_| WireError::Config {
            reason: "kubernetes: service list transport failed",
        })?;
    let endpoints = source
        .list_endpoints()
        .await
        .map_err(|_| WireError::Config {
            reason: "kubernetes: endpoints list transport failed",
        })?;
    kubernetes_from_api(zone, &services, &endpoints)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::message::Message;
    use crate::plugin::Chain;
    use crate::rr::{Rdata, RecordType};
    use crate::wire::Rcode;
    use std::net::{Ipv4Addr, Ipv6Addr};

    const SERVICES: &str = r#"{
        "items": [
            {
                "metadata": {"name": "web", "namespace": "default"},
                "spec": {
                    "type": "ClusterIP",
                    "clusterIP": "10.0.0.1",
                    "clusterIPs": ["10.0.0.1"],
                    "ports": [{"name": "http", "protocol": "TCP", "port": 80}]
                }
            },
            {
                "metadata": {"name": "db6", "namespace": "default"},
                "spec": {"type": "ClusterIP", "clusterIP": "fd00::1"}
            },
            {
                "metadata": {"name": "cache", "namespace": "default"},
                "spec": {"type": "ClusterIP", "clusterIP": "None"}
            },
            {
                "metadata": {"name": "ext", "namespace": "default"},
                "spec": {"type": "ExternalName", "externalName": "example.com"}
            }
        ]
    }"#;

    const ENDPOINTS: &str = r#"{
        "items": [
            {
                "metadata": {"name": "cache", "namespace": "default"},
                "subsets": [
                    {
                        "addresses": [
                            {"ip": "10.0.1.1", "hostname": "c0"},
                            {"ip": "10.0.1.2", "hostname": "c1"}
                        ],
                        "ports": [{"name": "http", "protocol": "TCP", "port": 80}]
                    }
                ]
            }
        ]
    }"#;

    fn ask(k: Kubernetes, name: &str, t: RecordType) -> Message {
        let chain = Chain::new(vec![Box::new(k)]);
        chain.handle(&Message::query(Name::parse(name).unwrap(), t, 1))
    }

    #[test]
    fn cluster_ip_service_resolves_to_its_ip() {
        let k = kubernetes_from_api("cluster.local", SERVICES, ENDPOINTS).unwrap();
        let m = ask(k, "web.default.svc.cluster.local", RecordType::A);
        assert_eq!(m.header.rcode, Rcode::NoError);
        assert_eq!(m.answers[0].rdata, Rdata::A(Ipv4Addr::new(10, 0, 0, 1)));
    }

    #[test]
    fn ipv6_cluster_ip_resolves_aaaa() {
        let k = kubernetes_from_api("cluster.local", SERVICES, ENDPOINTS).unwrap();
        let m = ask(k, "db6.default.svc.cluster.local", RecordType::Aaaa);
        assert_eq!(
            m.answers[0].rdata,
            Rdata::Aaaa(Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 1))
        );
    }

    #[test]
    fn headless_service_resolves_to_all_endpoint_ips() {
        let k = kubernetes_from_api("cluster.local", SERVICES, ENDPOINTS).unwrap();
        let m = ask(k, "cache.default.svc.cluster.local", RecordType::A);
        let ips: Vec<_> = m
            .answers
            .iter()
            .filter_map(|rr| match rr.rdata {
                Rdata::A(a) => Some(a),
                _ => None,
            })
            .collect();
        assert_eq!(ips.len(), 2);
        assert!(ips.contains(&Ipv4Addr::new(10, 0, 1, 1)));
        assert!(ips.contains(&Ipv4Addr::new(10, 0, 1, 2)));
    }

    #[test]
    fn headless_endpoint_hostname_resolves() {
        let k = kubernetes_from_api("cluster.local", SERVICES, ENDPOINTS).unwrap();
        let m = ask(k, "c0.cache.default.svc.cluster.local", RecordType::A);
        assert_eq!(m.answers[0].rdata, Rdata::A(Ipv4Addr::new(10, 0, 1, 1)));
    }

    #[test]
    fn external_name_service_resolves_to_cname() {
        let k = kubernetes_from_api("cluster.local", SERVICES, ENDPOINTS).unwrap();
        let m = ask(k, "ext.default.svc.cluster.local", RecordType::A);
        assert_eq!(
            m.answers[0].rdata,
            Rdata::Cname(Name::parse("example.com").unwrap())
        );
    }

    #[test]
    fn named_port_resolves_srv() {
        let k = kubernetes_from_api("cluster.local", SERVICES, ENDPOINTS).unwrap();
        let m = ask(
            k,
            "_http._tcp.web.default.svc.cluster.local",
            RecordType::Srv,
        );
        let srv = m.answers.iter().find_map(|rr| match &rr.rdata {
            Rdata::Srv { port, .. } => Some(*port),
            _ => None,
        });
        assert_eq!(srv, Some(80));
    }

    #[test]
    fn unknown_service_is_nxdomain() {
        let k = kubernetes_from_api("cluster.local", SERVICES, ENDPOINTS).unwrap();
        let m = ask(k, "ghost.default.svc.cluster.local", RecordType::A);
        assert_eq!(m.header.rcode, Rcode::NxDomain);
    }

    #[test]
    fn malformed_service_json_is_a_config_error() {
        assert!(matches!(
            kubernetes_from_api("cluster.local", "{ not json", ENDPOINTS),
            Err(WireError::Config { .. })
        ));
    }

    #[test]
    fn empty_endpoints_list_is_accepted() {
        let k = kubernetes_from_api("cluster.local", SERVICES, r#"{"items":[]}"#).unwrap();
        // Headless service with no endpoints: the name exists but has no
        // address records → NODATA, not a decode failure.
        let m = ask(k, "cache.default.svc.cluster.local", RecordType::A);
        assert_eq!(m.header.rcode, Rcode::NoError);
        assert!(m.answers.is_empty());
    }

    #[tokio::test]
    async fn static_source_feeds_the_converter() {
        let source = StaticSource {
            services: SERVICES.to_string(),
            endpoints: ENDPOINTS.to_string(),
        };
        let k = kubernetes_from_source("cluster.local", &source)
            .await
            .unwrap();
        let m = ask(k, "web.default.svc.cluster.local", RecordType::A);
        assert_eq!(m.answers[0].rdata, Rdata::A(Ipv4Addr::new(10, 0, 0, 1)));
    }
}
