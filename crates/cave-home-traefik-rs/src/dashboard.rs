// SPDX-License-Identifier: Apache-2.0
//! Dashboard status snapshot.
//!
//! Spec basis: Traefik's dashboard / API exposes the live routers, services and
//! middlewares as JSON. This builds a serializable snapshot of a validated
//! [`DynamicConfig`] for the dashboard UI / API endpoint to serve.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::config::DynamicConfig;

/// A router as shown on the dashboard.
#[derive(Debug, Clone, Serialize)]
pub struct RouterView {
    /// Router name.
    pub name: String,
    /// The matching rule text.
    pub rule: String,
    /// The service it forwards to.
    pub service: String,
    /// Whether TLS is enabled.
    pub tls: bool,
    /// The router's entrypoints.
    pub entrypoints: Vec<String>,
    /// Middlewares applied, in order.
    pub middlewares: Vec<String>,
    /// Explicit priority, if set.
    pub priority: Option<usize>,
}

/// A service as shown on the dashboard.
#[derive(Debug, Clone, Serialize)]
pub struct ServiceView {
    /// Service name.
    pub name: String,
    /// Backend server URLs.
    pub servers: Vec<String>,
    /// Count of currently-healthy servers.
    pub healthy: usize,
    /// Total server count.
    pub total: usize,
}

/// A full dashboard snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    /// All routers.
    pub routers: Vec<RouterView>,
    /// All referenced services.
    pub services: Vec<ServiceView>,
    /// All referenced middleware names.
    pub middlewares: Vec<String>,
}

impl Snapshot {
    /// Build a snapshot from a validated config.
    #[must_use]
    pub fn from_config(config: &DynamicConfig) -> Self {
        unimplemented!()
    }

    /// Serialize the snapshot to JSON.
    ///
    /// # Errors
    /// Returns the `serde_json` error if serialization fails (it should not for
    /// this plain data).
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loadbalancer::{LoadBalancer, Server, Service};
    use crate::router::Router;

    fn config() -> DynamicConfig {
        let router = Router::new("api", "Host(`app.example`) && PathPrefix(`/api`)", "api-svc")
            .unwrap()
            .with_tls(true);
        let service = Service::new(
            "api-svc",
            vec![
                Server::new("http://10.0.0.1:80"),
                Server::new("http://10.0.0.2:80").with_healthy(false),
            ],
            LoadBalancer::WeightedRoundRobin,
        );
        DynamicConfig::build(vec![router], vec![service], vec![]).unwrap()
    }

    #[test]
    fn snapshot_lists_routers_and_services() {
        let snap = Snapshot::from_config(&config());
        assert_eq!(snap.routers.len(), 1);
        assert_eq!(snap.routers[0].name, "api");
        assert!(snap.routers[0].tls);
        assert_eq!(snap.routers[0].service, "api-svc");

        assert_eq!(snap.services.len(), 1);
        assert_eq!(snap.services[0].name, "api-svc");
        assert_eq!(snap.services[0].total, 2);
        assert_eq!(snap.services[0].healthy, 1);
    }

    #[test]
    fn snapshot_serializes_to_json() {
        let json = Snapshot::from_config(&config()).to_json().unwrap();
        assert!(json.contains("\"api\""));
        assert!(json.contains("\"api-svc\""));
        assert!(json.contains("http://10.0.0.1:80"));
    }
}
