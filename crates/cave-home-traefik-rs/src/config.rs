// SPDX-License-Identifier: Apache-2.0
//! Dynamic-configuration snapshot + reference validation.
//!
//! A [`DynamicConfig`] is an immutable snapshot of the routers, services and
//! middlewares the ingress is currently serving. Before a snapshot is put into
//! service it is validated: every router must reference a known service and
//! only known middlewares, service/router/middleware names must be unique, and
//! every service must have at least one server.
//!
//! Spec basis: Traefik's dynamic configuration is a set of routers + services +
//! middlewares; routers reference services and middlewares by name, and a
//! dangling reference is a configuration error. The provider *watchers* that
//! produce these snapshots (file / Kubernetes CRD / Docker labels) are deferred
//! to phase-1b; this models the validated in-memory snapshot they feed.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt;

use crate::loadbalancer::Service;
use crate::middleware::MiddlewareChain;
use crate::request::RequestDescriptor;
use crate::router::{select, Router};

/// A validated dynamic-configuration snapshot.
#[derive(Debug, Clone, Default)]
pub struct DynamicConfig {
    routers: Vec<Router>,
    services: BTreeMap<String, Service>,
    middlewares: BTreeMap<String, MiddlewareChain>,
}

/// A configuration validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// A router references a service that is not defined.
    UnknownService {
        /// The offending router.
        router: String,
        /// The undefined service it referenced.
        service: String,
    },
    /// A router references a middleware that is not defined.
    UnknownMiddleware {
        /// The offending router.
        router: String,
        /// The undefined middleware it referenced.
        middleware: String,
    },
    /// Two routers share a name.
    DuplicateRouter(String),
    /// Two services share a name.
    DuplicateService(String),
    /// A service has no backend servers.
    EmptyService(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownService { router, service } => {
                write!(f, "router {router} references unknown service {service}")
            }
            Self::UnknownMiddleware { router, middleware } => {
                write!(f, "router {router} references unknown middleware {middleware}")
            }
            Self::DuplicateRouter(n) => write!(f, "duplicate router name: {n}"),
            Self::DuplicateService(n) => write!(f, "duplicate service name: {n}"),
            Self::EmptyService(n) => write!(f, "service {n} has no servers"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// The result of routing a request through a validated config: the selected
/// router and the service it points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route<'a> {
    /// The router that matched.
    pub router: &'a Router,
    /// The service the router forwards to.
    pub service: &'a Service,
}

impl DynamicConfig {
    /// Build and validate a configuration snapshot.
    ///
    /// # Errors
    /// Returns [`ConfigError`] on duplicate names, dangling service/middleware
    /// references, or a service with no servers.
    pub fn build(
        routers: Vec<Router>,
        services: Vec<Service>,
        middlewares: Vec<(String, MiddlewareChain)>,
    ) -> Result<Self, ConfigError> {
        // Unique + non-empty services.
        let mut service_map = BTreeMap::new();
        for svc in services {
            if svc.servers.is_empty() {
                return Err(ConfigError::EmptyService(svc.name));
            }
            if service_map.insert(svc.name.clone(), svc.clone()).is_some() {
                return Err(ConfigError::DuplicateService(svc.name));
            }
        }

        // Middlewares (last definition would shadow; treat re-def as fine but
        // keep a set of known names for reference checking).
        let mut mw_map = BTreeMap::new();
        for (name, chain) in middlewares {
            mw_map.insert(name, chain);
        }
        let known_mws: BTreeSet<&String> = mw_map.keys().collect();

        // Unique routers + reference checks.
        let mut seen_routers = BTreeSet::new();
        for r in &routers {
            if !seen_routers.insert(r.name.clone()) {
                return Err(ConfigError::DuplicateRouter(r.name.clone()));
            }
            if !service_map.contains_key(&r.service) {
                return Err(ConfigError::UnknownService {
                    router: r.name.clone(),
                    service: r.service.clone(),
                });
            }
            for mw in &r.middlewares {
                if !known_mws.contains(mw) {
                    return Err(ConfigError::UnknownMiddleware {
                        router: r.name.clone(),
                        middleware: mw.clone(),
                    });
                }
            }
        }

        Ok(Self { routers, services: service_map, middlewares: mw_map })
    }

    /// Look up a service by name.
    #[must_use]
    pub fn service(&self, name: &str) -> Option<&Service> {
        self.services.get(name)
    }

    /// Look up a middleware chain by name.
    #[must_use]
    pub fn middleware(&self, name: &str) -> Option<&MiddlewareChain> {
        self.middlewares.get(name)
    }

    /// The configured routers.
    #[must_use]
    pub fn routers(&self) -> &[Router] {
        &self.routers
    }

    /// Route a request: select the best router on `entrypoint` and resolve its
    /// service. Returns `None` if no router matches. Because the snapshot was
    /// validated at build time, a matched router's service always resolves.
    #[must_use]
    pub fn route(&self, req: &RequestDescriptor, entrypoint: Option<&str>) -> Option<Route<'_>> {
        let router = select(&self.routers, req, entrypoint)?;
        let service = self.services.get(&router.service)?;
        Some(Route { router, service })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loadbalancer::{LoadBalancer, Server};
    use crate::middleware::Middleware;

    fn svc(name: &str) -> Service {
        Service::new(name, vec![Server::new("http://b:80")], LoadBalancer::WeightedRoundRobin)
    }

    fn router(name: &str, rule: &str, service: &str) -> Router {
        Router::new(name, rule, service).unwrap()
    }

    #[test]
    fn valid_config_builds() {
        let cfg = DynamicConfig::build(
            vec![router("r", "Host(`a.com`)", "s")],
            vec![svc("s")],
            vec![],
        );
        assert!(cfg.is_ok());
    }

    #[test]
    fn unknown_service_is_rejected() {
        let err = DynamicConfig::build(
            vec![router("r", "Host(`a.com`)", "ghost")],
            vec![svc("s")],
            vec![],
        )
        .unwrap_err();
        assert_eq!(
            err,
            ConfigError::UnknownService { router: "r".to_string(), service: "ghost".to_string() }
        );
    }

    #[test]
    fn unknown_middleware_is_rejected() {
        let r = router("r", "Host(`a.com`)", "s").with_middlewares(&["ghost-mw"]);
        let err = DynamicConfig::build(vec![r], vec![svc("s")], vec![]).unwrap_err();
        assert_eq!(
            err,
            ConfigError::UnknownMiddleware {
                router: "r".to_string(),
                middleware: "ghost-mw".to_string(),
            }
        );
    }

    #[test]
    fn known_middleware_reference_is_accepted() {
        let r = router("r", "Host(`a.com`)", "s").with_middlewares(&["strip"]);
        let mw = MiddlewareChain::new(vec![Middleware::AddPrefix { prefix: "/x".to_string() }]);
        let cfg = DynamicConfig::build(vec![r], vec![svc("s")], vec![("strip".to_string(), mw)]);
        assert!(cfg.is_ok());
    }

    #[test]
    fn duplicate_router_name_is_rejected() {
        let err = DynamicConfig::build(
            vec![router("dup", "Host(`a.com`)", "s"), router("dup", "Host(`b.com`)", "s")],
            vec![svc("s")],
            vec![],
        )
        .unwrap_err();
        assert_eq!(err, ConfigError::DuplicateRouter("dup".to_string()));
    }

    #[test]
    fn duplicate_service_name_is_rejected() {
        let err = DynamicConfig::build(vec![], vec![svc("s"), svc("s")], vec![]).unwrap_err();
        assert_eq!(err, ConfigError::DuplicateService("s".to_string()));
    }

    #[test]
    fn empty_service_is_rejected() {
        let empty = Service::new("s", vec![], LoadBalancer::WeightedRoundRobin);
        let err = DynamicConfig::build(vec![], vec![empty], vec![]).unwrap_err();
        assert_eq!(err, ConfigError::EmptyService("s".to_string()));
    }

    #[test]
    fn route_resolves_router_and_service() {
        let cfg = DynamicConfig::build(
            vec![router("r", "Host(`a.com`)", "s")],
            vec![svc("s")],
            vec![],
        )
        .unwrap();
        let req = RequestDescriptor::new("GET", "http", "a.com", "/");
        let route = cfg.route(&req, None).unwrap();
        assert_eq!(route.router.name, "r");
        assert_eq!(route.service.name, "s");
    }

    #[test]
    fn route_returns_none_when_no_router_matches() {
        let cfg = DynamicConfig::build(
            vec![router("r", "Host(`a.com`)", "s")],
            vec![svc("s")],
            vec![],
        )
        .unwrap();
        let req = RequestDescriptor::new("GET", "http", "other.com", "/");
        assert!(cfg.route(&req, None).is_none());
    }
}
