// SPDX-License-Identifier: Apache-2.0
//! Gateway API `HTTPRoute` → dynamic-config translation.
//!
//! The Gateway API (`gateway.networking.k8s.io/v1`) is the successor to
//! Ingress. An `HTTPRoute` attaches to a Gateway and carries a list of rules;
//! each rule has a set of *matches* (path / method / headers, OR-combined) and
//! a set of weighted *backendRefs* the matched traffic is balanced across.
//!
//! This module is the **pure translation half** of Traefik's Gateway API
//! provider: it turns a parsed [`HttpRoute`] (or a set of them) into Traefik
//! [`Router`]s plus the weighted backend sets they forward to, ready to feed
//! [`DynamicConfig::build`]. As with [`crate::ingress`], the watch loop and
//! endpoint resolution stay deferred (phase-1b, ADR-004):
//! [`GatewayTranslation::into_config`] takes a caller-supplied resolver, so the
//! translation owns no I/O and is fully deterministic.
//!
//! ## Documented semantics (Gateway API spec + Traefik gateway provider)
//!
//! * Each `(rule, match)` pair becomes one [`Router`]. Within a match, the
//!   path / method / header conditions are AND-combined; the rule's several
//!   matches each yield their own router (the matches OR at the rule level).
//! * The route's `hostnames` become a single `Host(`a`,`b`)` clause (a Traefik
//!   multi-value matcher ORs its arguments); an empty hostname list adds no
//!   host constraint.
//! * Path match type maps `Exact` → `Path` and `PathPrefix` → `PathPrefix`.
//!   `method` → `Method`, and each header match → `Header(name, value)`.
//! * A rule with no matches matches everything (`PathPrefix(`/`)` when there is
//!   also no hostname).
//! * A rule's `backendRefs` form **one** service whose servers carry each
//!   backend's `weight`, so weighted round-robin distributes traffic across the
//!   backends in proportion (Gateway API backend weighting).

use crate::config::{ConfigError, DynamicConfig};
use crate::loadbalancer::{LoadBalancer, Server, Service};
use crate::router::Router;
use crate::rule::ParseError;

/// A Gateway API path match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GwPathMatch {
    /// `type: Exact` — the path must equal `value`.
    Exact(String),
    /// `type: PathPrefix` — the path must be under the `value` prefix.
    PathPrefix(String),
}

impl GwPathMatch {
    /// `(matcher name, value)` for rule-text construction.
    fn clause(&self) -> (&'static str, &str) {
        match self {
            Self::Exact(v) => ("Path", v),
            Self::PathPrefix(v) => ("PathPrefix", v),
        }
    }
}

/// A Gateway API header match (`type: Exact`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GwHeaderMatch {
    /// Header name (case-insensitive per RFC 9110).
    pub name: String,
    /// Required header value.
    pub value: String,
}

impl GwHeaderMatch {
    /// Build a header match.
    #[must_use]
    pub fn new(name: &str, value: &str) -> Self {
        Self { name: name.to_string(), value: value.to_string() }
    }
}

/// One `HTTPRouteMatch`: path + method + headers, AND-combined.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HttpRouteMatch {
    /// Optional path match.
    pub path: Option<GwPathMatch>,
    /// Optional method match (e.g. `GET`).
    pub method: Option<String>,
    /// Header matches (all must hold).
    pub headers: Vec<GwHeaderMatch>,
}

impl HttpRouteMatch {
    /// A match on a path only.
    #[must_use]
    pub const fn path(path: GwPathMatch) -> Self {
        Self { path: Some(path), method: None, headers: Vec::new() }
    }
}

/// A weighted backend reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpBackendRef {
    /// Target `Service` name (in the route's namespace).
    pub service_name: String,
    /// Target port (1..=65535).
    pub port: u16,
    /// Relative weight; `0` means "receive no traffic" (Gateway API allows it).
    pub weight: u32,
}

impl HttpBackendRef {
    /// A backend with the default weight of 1.
    #[must_use]
    pub fn new(service_name: &str, port: u16) -> Self {
        Self { service_name: service_name.to_string(), port, weight: 1 }
    }

    /// Builder: set the weight.
    #[must_use]
    pub const fn with_weight(mut self, weight: u32) -> Self {
        self.weight = weight;
        self
    }

    fn validate(&self) -> Result<(), GatewayError> {
        if self.service_name.is_empty() {
            return Err(GatewayError::EmptyBackendName);
        }
        if self.port == 0 {
            return Err(GatewayError::InvalidPort(self.service_name.clone()));
        }
        Ok(())
    }
}

/// One `HTTPRouteRule`: matches (OR) + weighted backends (one service).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRouteRule {
    /// Match conditions; empty = match everything.
    pub matches: Vec<HttpRouteMatch>,
    /// Weighted backends the matched traffic is balanced across.
    pub backend_refs: Vec<HttpBackendRef>,
}

/// A Gateway API `HTTPRoute`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRoute {
    /// Namespace the route (and its backends) live in.
    pub namespace: String,
    /// Route object name.
    pub name: String,
    /// `spec.hostnames`; empty matches any host.
    pub hostnames: Vec<String>,
    /// `spec.rules`.
    pub rules: Vec<HttpRouteRule>,
}

/// An error translating an `HTTPRoute`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatewayError {
    /// A rule has no backendRefs (and filters are not yet modelled).
    NoBackend {
        /// The offending route (`namespace/name`).
        route: String,
    },
    /// A backendRef has an empty service name.
    EmptyBackendName,
    /// A backendRef names an invalid (zero) port.
    InvalidPort(String),
    /// A hostname/match value produced an unparseable rule string.
    InvalidRule {
        /// The offending route (`namespace/name`).
        route: String,
        /// The generated rule text that failed to parse.
        rule: String,
    },
    /// The generated routing config failed reference validation.
    Config(ConfigError),
}

impl std::fmt::Display for GatewayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoBackend { route } => write!(f, "httproute {route} has a rule with no backends"),
            Self::EmptyBackendName => write!(f, "httproute backend has an empty service name"),
            Self::InvalidPort(svc) => write!(f, "httproute backend {svc} has an invalid port"),
            Self::InvalidRule { route, rule } => {
                write!(f, "httproute {route} produced an unparseable rule: {rule}")
            }
            Self::Config(e) => write!(f, "httproute config invalid: {e}"),
        }
    }
}

impl std::error::Error for GatewayError {}

impl From<ConfigError> for GatewayError {
    fn from(e: ConfigError) -> Self {
        Self::Config(e)
    }
}

/// One service a translation produces: its Traefik id and the weighted backends
/// it aggregates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GwServiceRef {
    /// Generated Traefik service id.
    pub service_id: String,
    /// The weighted backends whose endpoints make up this service.
    pub backends: Vec<HttpBackendRef>,
}

/// The result of translating one or more `HTTPRoute`s.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GatewayTranslation {
    /// Generated routers, in deterministic order.
    pub routers: Vec<Router>,
    /// The distinct services the routers reference (one per rule).
    pub services: Vec<GwServiceRef>,
}

impl GatewayTranslation {
    /// Resolve every backend into [`Server`]s via `resolve` and build a
    /// validated [`DynamicConfig`]. Each backend's servers inherit that
    /// backend's `weight`, so weighted round-robin distributes traffic across
    /// the backends of a service in proportion.
    ///
    /// # Errors
    /// Returns [`ConfigError`] when the generated config fails validation (e.g.
    /// a service whose backends all resolve to zero servers).
    pub fn into_config<F>(self, resolve: F) -> Result<DynamicConfig, ConfigError>
    where
        F: Fn(&HttpBackendRef) -> Vec<Server>,
    {
        let services: Vec<Service> = self
            .services
            .iter()
            .map(|s| {
                let servers: Vec<Server> = s
                    .backends
                    .iter()
                    .flat_map(|b| resolve(b).into_iter().map(|srv| srv.with_weight(b.weight)))
                    .collect();
                Service::new(&s.service_id, servers, LoadBalancer::WeightedRoundRobin)
            })
            .collect();
        DynamicConfig::build(self.routers, services, vec![])
    }
}

/// Backtick-quote a rule-matcher argument.
fn quote(arg: &str) -> String {
    format!("`{arg}`")
}

impl HttpRoute {
    /// The `namespace/name` identifier used in diagnostics.
    #[must_use]
    pub fn id(&self) -> String {
        format!("{}/{}", self.namespace, self.name)
    }

    /// The shared `Host(...)` clause for this route's hostnames, or `None` when
    /// the route has no hostnames.
    fn host_clause(&self) -> Option<String> {
        if self.hostnames.is_empty() {
            return None;
        }
        let args: Vec<String> = self.hostnames.iter().map(|h| quote(h)).collect();
        Some(format!("Host({})", args.join(", ")))
    }

    /// Build a router, mapping a rule parse failure to a [`GatewayError`].
    fn router(&self, name: &str, rule_text: &str, service_id: &str) -> Result<Router, GatewayError> {
        Router::new(name, rule_text, service_id).map_err(|_: ParseError| GatewayError::InvalidRule {
            route: self.id(),
            rule: rule_text.to_string(),
        })
    }

    /// Translate this route into routers + weighted service references.
    ///
    /// # Errors
    /// Returns [`GatewayError`] for a rule with no backends, an invalid backend,
    /// or a hostname/match that yields an unparseable rule.
    pub fn translate(&self) -> Result<GatewayTranslation, GatewayError> {
        let mut out = GatewayTranslation::default();
        self.translate_into(&mut out)?;
        Ok(out)
    }

    fn translate_into(&self, out: &mut GatewayTranslation) -> Result<(), GatewayError> {
        let host_clause = self.host_clause();

        for (ri, rule) in self.rules.iter().enumerate() {
            if rule.backend_refs.is_empty() {
                return Err(GatewayError::NoBackend { route: self.id() });
            }
            for b in &rule.backend_refs {
                b.validate()?;
            }

            let service_id = format!("{}-{}-{}", self.namespace, self.name, ri);

            // No matches => the rule matches everything.
            let matches: Vec<HttpRouteMatch> =
                if rule.matches.is_empty() { vec![HttpRouteMatch::default()] } else { rule.matches.clone() };

            for (mi, m) in matches.iter().enumerate() {
                let mut clauses = Vec::new();
                if let Some(h) = &host_clause {
                    clauses.push(h.clone());
                }
                if let Some(p) = &m.path {
                    let (kind, value) = p.clause();
                    clauses.push(format!("{kind}({})", quote(value)));
                }
                if let Some(method) = &m.method {
                    clauses.push(format!("Method({})", quote(method)));
                }
                for h in &m.headers {
                    clauses.push(format!("Header({}, {})", quote(&h.name), quote(&h.value)));
                }
                if clauses.is_empty() {
                    // No host and an empty match: catch-all.
                    clauses.push("PathPrefix(`/`)".to_string());
                }
                let rule_text = clauses.join(" && ");

                let name = format!("{}-{}-{ri}-{mi}", self.namespace, self.name);
                let router = self.router(&name, &rule_text, &service_id)?;
                out.routers.push(router);
            }

            out.services.push(GwServiceRef { service_id, backends: rule.backend_refs.clone() });
        }
        Ok(())
    }
}

/// Translate a set of `HTTPRoute`s into one merged [`GatewayTranslation`]
/// (routers concatenated in input order, services concatenated per route/rule).
///
/// # Errors
/// Returns the first [`GatewayError`] encountered.
pub fn translate_routes(routes: &[HttpRoute]) -> Result<GatewayTranslation, GatewayError> {
    let mut out = GatewayTranslation::default();
    for route in routes {
        route.translate_into(&mut out)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::RequestDescriptor;

    fn route(rules: Vec<HttpRouteRule>) -> HttpRoute {
        HttpRoute {
            namespace: "default".to_string(),
            name: "rt".to_string(),
            hostnames: vec![],
            rules,
        }
    }

    fn rule(matches: Vec<HttpRouteMatch>, backends: Vec<HttpBackendRef>) -> HttpRouteRule {
        HttpRouteRule { matches, backend_refs: backends }
    }

    #[test]
    fn host_and_path_build_combined_rule() {
        let mut r = route(vec![rule(
            vec![HttpRouteMatch::path(GwPathMatch::PathPrefix("/api".to_string()))],
            vec![HttpBackendRef::new("api", 80)],
        )]);
        r.hostnames = vec!["example.com".to_string()];
        let t = r.translate().unwrap();
        assert_eq!(t.routers.len(), 1);
        assert_eq!(t.routers[0].rule_text, "Host(`example.com`) && PathPrefix(`/api`)");
        assert_eq!(t.routers[0].service, "default-rt-0");
    }

    #[test]
    fn exact_path_uses_path_matcher() {
        let r = route(vec![rule(
            vec![HttpRouteMatch::path(GwPathMatch::Exact("/x".to_string()))],
            vec![HttpBackendRef::new("a", 80)],
        )]);
        assert_eq!(r.translate().unwrap().routers[0].rule_text, "Path(`/x`)");
    }

    #[test]
    fn multiple_hostnames_become_or_list() {
        let mut r = route(vec![rule(vec![], vec![HttpBackendRef::new("a", 80)])]);
        r.hostnames = vec!["a.com".to_string(), "b.com".to_string()];
        let t = r.translate().unwrap();
        assert_eq!(t.routers[0].rule_text, "Host(`a.com`, `b.com`)");
    }

    #[test]
    fn method_and_headers_are_anded() {
        let m = HttpRouteMatch {
            path: Some(GwPathMatch::PathPrefix("/".to_string())),
            method: Some("GET".to_string()),
            headers: vec![GwHeaderMatch::new("X-Env", "prod")],
        };
        let r = route(vec![rule(vec![m], vec![HttpBackendRef::new("a", 80)])]);
        let t = r.translate().unwrap();
        assert_eq!(
            t.routers[0].rule_text,
            "PathPrefix(`/`) && Method(`GET`) && Header(`X-Env`, `prod`)"
        );
    }

    #[test]
    fn empty_match_with_no_host_is_catch_all() {
        let r = route(vec![rule(vec![], vec![HttpBackendRef::new("a", 80)])]);
        assert_eq!(r.translate().unwrap().routers[0].rule_text, "PathPrefix(`/`)");
    }

    #[test]
    fn host_with_empty_match_omits_path() {
        let mut r = route(vec![rule(vec![], vec![HttpBackendRef::new("a", 80)])]);
        r.hostnames = vec!["only.com".to_string()];
        assert_eq!(r.translate().unwrap().routers[0].rule_text, "Host(`only.com`)");
    }

    #[test]
    fn several_matches_make_several_routers_sharing_one_service() {
        let r = route(vec![rule(
            vec![
                HttpRouteMatch::path(GwPathMatch::PathPrefix("/a".to_string())),
                HttpRouteMatch::path(GwPathMatch::PathPrefix("/b".to_string())),
            ],
            vec![HttpBackendRef::new("svc", 80)],
        )]);
        let t = r.translate().unwrap();
        assert_eq!(t.routers.len(), 2);
        assert_eq!(t.routers[0].service, t.routers[1].service);
        assert_ne!(t.routers[0].name, t.routers[1].name);
        assert_eq!(t.services.len(), 1);
        assert_eq!(t.services[0].backends.len(), 1);
    }

    #[test]
    fn rule_without_backends_is_rejected() {
        let r = route(vec![rule(
            vec![HttpRouteMatch::path(GwPathMatch::PathPrefix("/".to_string()))],
            vec![],
        )]);
        assert_eq!(
            r.translate().unwrap_err(),
            GatewayError::NoBackend { route: "default/rt".to_string() }
        );
    }

    #[test]
    fn empty_backend_name_and_zero_port_are_rejected() {
        let r1 = route(vec![rule(vec![], vec![HttpBackendRef::new("", 80)])]);
        assert_eq!(r1.translate().unwrap_err(), GatewayError::EmptyBackendName);
        let r2 = route(vec![rule(vec![], vec![HttpBackendRef::new("a", 0)])]);
        assert_eq!(r2.translate().unwrap_err(), GatewayError::InvalidPort("a".to_string()));
    }

    #[test]
    fn backtick_hostname_is_reported_not_panicked() {
        let mut r = route(vec![rule(vec![], vec![HttpBackendRef::new("a", 80)])]);
        r.hostnames = vec!["ev`il".to_string()];
        match r.translate().unwrap_err() {
            GatewayError::InvalidRule { route, .. } => assert_eq!(route, "default/rt"),
            other => panic!("expected InvalidRule, got {other:?}"),
        }
    }

    #[test]
    fn into_config_distributes_across_weighted_backends() {
        // Two backends, weights 3 and 1: of every 4 requests, 3 go to the
        // first backend's server and 1 to the second's.
        let r = route(vec![rule(
            vec![HttpRouteMatch::path(GwPathMatch::PathPrefix("/".to_string()))],
            vec![
                HttpBackendRef::new("heavy", 80).with_weight(3),
                HttpBackendRef::new("light", 80).with_weight(1),
            ],
        )]);
        let cfg = r
            .translate()
            .unwrap()
            .into_config(|b| vec![Server::new(&format!("http://{}:80", b.service_name))])
            .unwrap();
        let svc = cfg.service("default-rt-0").unwrap();
        let picks: Vec<&str> =
            (0..4).map(|i| svc.pick_round_robin(i).unwrap().url.as_str()).collect();
        let heavy = picks.iter().filter(|u| u.contains("heavy")).count();
        let light = picks.iter().filter(|u| u.contains("light")).count();
        assert_eq!((heavy, light), (3, 1));
    }

    #[test]
    fn into_config_is_routable_end_to_end() {
        let mut r = route(vec![rule(
            vec![HttpRouteMatch::path(GwPathMatch::PathPrefix("/api".to_string()))],
            vec![HttpBackendRef::new("api", 80)],
        )]);
        r.hostnames = vec!["example.com".to_string()];
        let cfg = r
            .translate()
            .unwrap()
            .into_config(|_b| vec![Server::new("http://10.0.0.1:80")])
            .unwrap();
        let req = RequestDescriptor::new("GET", "http", "example.com", "/api/x");
        assert_eq!(cfg.route(&req, None).unwrap().service.name, "default-rt-0");
    }

    #[test]
    fn translate_routes_merges_multiple_routes() {
        let mut a = route(vec![rule(vec![], vec![HttpBackendRef::new("a", 80)])]);
        a.name = "a".to_string();
        a.hostnames = vec!["a.com".to_string()];
        let mut b = route(vec![rule(vec![], vec![HttpBackendRef::new("b", 80)])]);
        b.name = "b".to_string();
        b.hostnames = vec!["b.com".to_string()];
        let t = translate_routes(&[a, b]).unwrap();
        assert_eq!(t.routers.len(), 2);
        assert_eq!(t.services.len(), 2);
        assert_eq!(t.routers[0].rule_text, "Host(`a.com`)");
        assert_eq!(t.routers[1].rule_text, "Host(`b.com`)");
    }
}
