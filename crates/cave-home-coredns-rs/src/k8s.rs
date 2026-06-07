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

/// The abstract Kubernetes API connection: the source of the `Service` and
/// `Endpoints` lists the indexer converts. The live watching client is the
/// deferred I/O shell; this seam lets the conversion be tested without one.
pub trait ApiSource {
    /// Fetch the current `ServiceList` JSON (`/api/v1/services`).
    fn list_services(&self)
    -> impl std::future::Future<Output = std::io::Result<String>> + Send;
    /// Fetch the current `Endpoints` list JSON (`/api/v1/endpoints`).
    fn list_endpoints(&self)
    -> impl std::future::Future<Output = std::io::Result<String>> + Send;
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
pub async fn kubernetes_from_source(zone: &str, source: &impl ApiSource) -> Result<Kubernetes> {
    let services = source.list_services().await.map_err(|_| WireError::Config {
        reason: "kubernetes: service list transport failed",
    })?;
    let endpoints = source.list_endpoints().await.map_err(|_| WireError::Config {
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
        let k = kubernetes_from_source("cluster.local", &source).await.unwrap();
        let m = ask(k, "web.default.svc.cluster.local", RecordType::A);
        assert_eq!(m.answers[0].rdata, Rdata::A(Ipv4Addr::new(10, 0, 0, 1)));
    }
}
