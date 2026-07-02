// SPDX-License-Identifier: Apache-2.0
//! Kubernetes `Ingress` → dynamic-config translation.
//!
//! An ingress controller's job is to turn declarative Kubernetes `Ingress`
//! resources (`networking.k8s.io/v1`) into the concrete routers + services the
//! proxy serves. This module is the **pure translation half** of Traefik's
//! `kubernetes-ingress` provider: given a parsed [`Ingress`] (or a set of
//! them), it produces [`Router`]s and the backend references they point at,
//! ready to feed [`DynamicConfig::build`].
//!
//! The *watch loop* that lists/watches `Ingress` objects from the API server,
//! and the *endpoint resolution* that turns a backend `Service`+port into real
//! server URLs, are I/O- and cluster-bound and stay deferred to phase-1b
//! (ADR-004). They are kept out by design: [`Translation::into_config`] takes a
//! caller-supplied resolver closure, so the translation itself is std-only and
//! fully deterministic.
//!
//! ## Documented semantics (Traefik kubernetes-ingress provider + k8s Ingress)
//!
//! * Each `(rule host, path)` pair becomes one [`Router`]. The router rule is
//!   `Host(`h`) && <PathMatcher>(`p`)`, omitting whichever part is absent.
//! * `pathType` selects the path matcher: `Exact` → `Path`, `Prefix` →
//!   `PathPrefix`, and `ImplementationSpecific` → `PathPrefix` (Traefik's
//!   documented default for the implementation-specific type).
//! * A backend `Service`+port maps to a Traefik service named
//!   `<namespace>-<service>-<port>` (the port is its number, or its name for a
//!   named target port).
//! * `spec.tls` enables TLS for a router whose host is listed (an entry with no
//!   hosts is a wildcard covering every host).
//! * `spec.defaultBackend` becomes a catch-all `PathPrefix(`/`)` router with the
//!   lowest priority, so it only wins when no host/path rule matches.
//! * `spec.ingressClassName` filters which ingresses a controller handles
//!   ([`Ingress::handled_by`]).

use crate::config::{ConfigError, DynamicConfig};
use crate::loadbalancer::{LoadBalancer, Server, Service};
use crate::router::Router;
use crate::rule::ParseError;

/// The port of a backend service: a numeric port or a named target port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServicePort {
    /// A numeric service port (1..=65535).
    Number(u16),
    /// A named service port (resolved to a number by the endpoint layer).
    Name(String),
}

impl ServicePort {
    /// The slug used in the generated Traefik service name.
    fn slug(&self) -> String {
        match self {
            Self::Number(n) => n.to_string(),
            Self::Name(name) => name.clone(),
        }
    }
}

/// A Kubernetes `IngressBackend`: the target service + port for a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngressBackend {
    /// The target `Service` name (within the ingress's namespace).
    pub service_name: String,
    /// The target service port.
    pub service_port: ServicePort,
}

impl IngressBackend {
    /// Convenience constructor for a numeric-port backend.
    #[must_use]
    pub fn numeric(service_name: &str, port: u16) -> Self {
        Self { service_name: service_name.to_string(), service_port: ServicePort::Number(port) }
    }

    /// Convenience constructor for a named-port backend.
    #[must_use]
    pub fn named(service_name: &str, port_name: &str) -> Self {
        Self {
            service_name: service_name.to_string(),
            service_port: ServicePort::Name(port_name.to_string()),
        }
    }

    /// The generated Traefik service id: `<namespace>-<service>-<port>`.
    #[must_use]
    pub fn service_id(&self, namespace: &str) -> String {
        format!("{namespace}-{}-{}", self.service_name, self.service_port.slug())
    }

    /// Validate the backend (non-empty service name, non-zero numeric port).
    fn validate(&self) -> Result<(), IngressError> {
        if self.service_name.is_empty() {
            return Err(IngressError::EmptyServiceName);
        }
        if self.service_port == ServicePort::Number(0) {
            return Err(IngressError::InvalidPort(self.service_name.clone()));
        }
        Ok(())
    }
}

/// How a path is matched, mirroring Kubernetes `pathType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathType {
    /// Match the request path exactly (`Path`).
    Exact,
    /// Match a leading path segment prefix (`PathPrefix`).
    Prefix,
    /// Implementation-specific; Traefik treats this as `PathPrefix`.
    ImplementationSpecific,
}

impl PathType {
    /// The rule-grammar matcher name this path type maps to.
    const fn matcher(self) -> &'static str {
        match self {
            Self::Exact => "Path",
            Self::Prefix | Self::ImplementationSpecific => "PathPrefix",
        }
    }
}

/// A single `HTTPIngressPath`: path + match type + backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpPath {
    /// The path to match; `None` (or empty) means "no path constraint".
    pub path: Option<String>,
    /// How `path` is matched.
    pub path_type: PathType,
    /// The backend this path forwards to.
    pub backend: IngressBackend,
}

impl HttpPath {
    /// Build a path entry.
    #[must_use]
    pub fn new(path: Option<&str>, path_type: PathType, backend: IngressBackend) -> Self {
        Self { path: path.map(str::to_string), path_type, backend }
    }

    /// The effective path string, treating an empty string as absent.
    fn effective_path(&self) -> Option<&str> {
        self.path.as_deref().filter(|p| !p.is_empty())
    }
}

/// A Kubernetes `IngressRule`: an optional host and its HTTP paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngressRule {
    /// The host this rule applies to; `None` matches any host.
    pub host: Option<String>,
    /// The HTTP paths under this host.
    pub paths: Vec<HttpPath>,
}

impl IngressRule {
    /// Build a rule.
    #[must_use]
    pub fn new(host: Option<&str>, paths: Vec<HttpPath>) -> Self {
        Self { host: host.map(str::to_string), paths }
    }
}

/// A Kubernetes `IngressTLS` entry: the hosts a secret terminates TLS for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngressTls {
    /// Hosts this TLS config covers; empty = wildcard (every host).
    pub hosts: Vec<String>,
    /// The `Secret` holding the certificate (modelled; acquisition deferred).
    pub secret_name: Option<String>,
}

impl IngressTls {
    /// Whether this entry terminates TLS for `host`. An empty host list is a
    /// wildcard. Host comparison is case-insensitive (RFC 4343 / DNS).
    #[must_use]
    pub fn covers(&self, host: Option<&str>) -> bool {
        if self.hosts.is_empty() {
            return true;
        }
        host.is_some_and(|h| self.hosts.iter().any(|t| t.eq_ignore_ascii_case(h)))
    }
}

/// A Kubernetes `Ingress` resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ingress {
    /// The namespace the ingress (and its backend services) live in.
    pub namespace: String,
    /// The ingress object name.
    pub name: String,
    /// `spec.ingressClassName`; `None` is the "no class" (default) ingress.
    pub ingress_class_name: Option<String>,
    /// `spec.defaultBackend`: the fallback when no rule matches.
    pub default_backend: Option<IngressBackend>,
    /// `spec.rules`.
    pub rules: Vec<IngressRule>,
    /// `spec.tls`.
    pub tls: Vec<IngressTls>,
}

/// An error translating an `Ingress` into routing config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngressError {
    /// A rule produced no matcher at all (no host and no path).
    EmptyRule {
        /// The offending ingress (`namespace/name`).
        ingress: String,
    },
    /// A backend has an empty service name.
    EmptyServiceName,
    /// A backend names a zero / invalid numeric port.
    InvalidPort(String),
    /// A host/path value produced a rule string the grammar rejects (e.g. a
    /// host containing a backtick). The translation is reported, not panicked.
    InvalidRule {
        /// The offending ingress (`namespace/name`).
        ingress: String,
        /// The generated rule text that failed to parse.
        rule: String,
    },
    /// The generated routing config failed reference validation.
    Config(ConfigError),
}

impl std::fmt::Display for IngressError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyRule { ingress } => {
                write!(f, "ingress {ingress} has a rule with neither host nor path")
            }
            Self::EmptyServiceName => write!(f, "ingress backend has an empty service name"),
            Self::InvalidPort(svc) => write!(f, "ingress backend {svc} has an invalid port"),
            Self::InvalidRule { ingress, rule } => {
                write!(f, "ingress {ingress} produced an unparseable rule: {rule}")
            }
            Self::Config(e) => write!(f, "ingress config invalid: {e}"),
        }
    }
}

impl std::error::Error for IngressError {}

impl From<ConfigError> for IngressError {
    fn from(e: ConfigError) -> Self {
        Self::Config(e)
    }
}

/// One backend a translation references: the generated Traefik service id and
/// the Kubernetes backend it resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendRef {
    /// The generated Traefik service id (`<namespace>-<service>-<port>`).
    pub service_id: String,
    /// The Kubernetes backend (service + port) it maps to.
    pub backend: IngressBackend,
}

/// The result of translating one or more ingresses: the routers and the
/// distinct backends they reference (endpoint resolution is the caller's job).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Translation {
    /// The generated routers, in deterministic order.
    pub routers: Vec<Router>,
    /// The distinct backends the routers reference, deduplicated by service id
    /// and ordered by first appearance.
    pub backends: Vec<BackendRef>,
}

impl Translation {
    /// Record a backend reference, keeping the set distinct by service id.
    fn add_backend(&mut self, service_id: String, backend: IngressBackend) {
        if self.backends.iter().any(|b| b.service_id == service_id) {
            return;
        }
        self.backends.push(BackendRef { service_id, backend });
    }

    /// Resolve the referenced backends into [`Service`]s via `resolve` and build
    /// a validated [`DynamicConfig`].
    ///
    /// `resolve` turns a Kubernetes backend into its concrete backend servers
    /// (the endpoint layer the controller owns). A backend that resolves to no
    /// servers yields a [`ConfigError::EmptyService`].
    ///
    /// # Errors
    /// Returns [`ConfigError`] when the generated config fails validation (e.g.
    /// a backend resolves to zero servers, or duplicate router names collide).
    pub fn into_config<F>(self, resolve: F) -> Result<DynamicConfig, ConfigError>
    where
        F: Fn(&IngressBackend) -> Vec<Server>,
    {
        let services: Vec<Service> = self
            .backends
            .iter()
            .map(|b| {
                Service::new(&b.service_id, resolve(&b.backend), LoadBalancer::WeightedRoundRobin)
            })
            .collect();
        DynamicConfig::build(self.routers, services, vec![])
    }
}

/// Lower-case slug of `s`, replacing every non-alphanumeric char with `-`.
fn slug(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect()
}

/// Backtick-quote a rule-matcher argument.
fn quote(arg: &str) -> String {
    format!("`{arg}`")
}

impl Ingress {
    /// The `namespace/name` identifier used in diagnostics.
    #[must_use]
    pub fn id(&self) -> String {
        format!("{}/{}", self.namespace, self.name)
    }

    /// Whether a controller with class `controller_class` should handle this
    /// ingress.
    ///
    /// * A controller with no configured class handles every ingress.
    /// * A controller with a class handles only ingresses that name that class.
    #[must_use]
    pub fn handled_by(&self, controller_class: Option<&str>) -> bool {
        controller_class.is_none_or(|cls| self.ingress_class_name.as_deref() == Some(cls))
    }

    /// Whether TLS should be enabled for a router on `host` given `spec.tls`.
    fn tls_for(&self, host: Option<&str>) -> bool {
        self.tls.iter().any(|t| t.covers(host))
    }

    /// Translate this single ingress into routers + backend references.
    ///
    /// # Errors
    /// Returns [`IngressError`] for an empty service name, an invalid port, or a
    /// rule that yields no matcher (no host and no path).
    pub fn translate(&self) -> Result<Translation, IngressError> {
        let mut out = Translation::default();
        self.translate_into(&mut out)?;
        Ok(out)
    }

    /// Build a router, mapping a rule parse failure to an [`IngressError`].
    fn router(&self, name: &str, rule_text: &str, service_id: &str) -> Result<Router, IngressError> {
        Router::new(name, rule_text, service_id).map_err(|_: ParseError| IngressError::InvalidRule {
            ingress: self.id(),
            rule: rule_text.to_string(),
        })
    }

    /// Append this ingress's routers/backends into `out`.
    fn translate_into(&self, out: &mut Translation) -> Result<(), IngressError> {
        for rule in &self.rules {
            let host = rule.host.as_deref();
            let tls = self.tls_for(host);
            for path in &rule.paths {
                path.backend.validate()?;
                let eff_path = path.effective_path();

                // Build the rule text from whichever of host/path is present.
                let mut clauses = Vec::new();
                if let Some(h) = host {
                    clauses.push(format!("Host({})", quote(h)));
                }
                if let Some(p) = eff_path {
                    clauses.push(format!("{}({})", path.path_type.matcher(), quote(p)));
                }
                if clauses.is_empty() {
                    return Err(IngressError::EmptyRule { ingress: self.id() });
                }
                let rule_text = clauses.join(" && ");

                let service_id = path.backend.service_id(&self.namespace);
                let name = format!(
                    "{}-{}-{}-{}",
                    self.namespace,
                    self.name,
                    slug(host.unwrap_or("wildcard")),
                    slug(eff_path.unwrap_or("root")),
                );
                let router = self.router(&name, &rule_text, &service_id)?.with_tls(tls);
                out.routers.push(router);
                out.add_backend(service_id, path.backend.clone());
            }
        }

        if let Some(backend) = &self.default_backend {
            backend.validate()?;
            let service_id = backend.service_id(&self.namespace);
            let name = format!("{}-{}-default", self.namespace, self.name);
            // Lowest priority so the catch-all only wins when nothing else does.
            let router = self
                .router(&name, "PathPrefix(`/`)", &service_id)?
                .with_priority(1)
                .with_tls(self.tls_for(None));
            out.routers.push(router);
            out.add_backend(service_id, backend.clone());
        }

        Ok(())
    }
}

/// Translate a set of ingresses handled by `controller_class` into one merged
/// [`Translation`] (routers concatenated in input order, backends deduplicated).
///
/// Ingresses the controller does not handle ([`Ingress::handled_by`]) are
/// skipped.
///
/// # Errors
/// Returns the first [`IngressError`] encountered while translating a handled
/// ingress.
pub fn translate_ingresses(
    ingresses: &[Ingress],
    controller_class: Option<&str>,
) -> Result<Translation, IngressError> {
    let mut out = Translation::default();
    for ing in ingresses {
        if ing.handled_by(controller_class) {
            ing.translate_into(&mut out)?;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::RequestDescriptor;

    fn prefix_path(path: &str, backend: IngressBackend) -> HttpPath {
        HttpPath::new(Some(path), PathType::Prefix, backend)
    }

    fn ingress(rules: Vec<IngressRule>) -> Ingress {
        Ingress {
            namespace: "default".to_string(),
            name: "web".to_string(),
            ingress_class_name: None,
            default_backend: None,
            rules,
            tls: vec![],
        }
    }

    #[test]
    fn service_id_uses_numeric_port() {
        let b = IngressBackend::numeric("api", 8080);
        assert_eq!(b.service_id("default"), "default-api-8080");
    }

    #[test]
    fn service_id_uses_named_port() {
        let b = IngressBackend::named("api", "http");
        assert_eq!(b.service_id("prod"), "prod-api-http");
    }

    #[test]
    fn handled_by_no_controller_class_accepts_all() {
        let mut ing = ingress(vec![]);
        ing.ingress_class_name = Some("nginx".to_string());
        assert!(ing.handled_by(None));
    }

    #[test]
    fn handled_by_matches_class() {
        let mut ing = ingress(vec![]);
        ing.ingress_class_name = Some("traefik".to_string());
        assert!(ing.handled_by(Some("traefik")));
        assert!(!ing.handled_by(Some("nginx")));
    }

    #[test]
    fn handled_by_no_ingress_class_rejected_when_controller_has_class() {
        let ing = ingress(vec![]); // ingress_class_name = None
        assert!(!ing.handled_by(Some("traefik")));
    }

    #[test]
    fn host_and_prefix_path_build_combined_rule() {
        let ing = ingress(vec![IngressRule::new(
            Some("example.com"),
            vec![prefix_path("/api", IngressBackend::numeric("api", 80))],
        )]);
        let t = ing.translate().unwrap();
        assert_eq!(t.routers.len(), 1);
        assert_eq!(t.routers[0].rule_text, "Host(`example.com`) && PathPrefix(`/api`)");
        assert_eq!(t.routers[0].service, "default-api-80");
    }

    #[test]
    fn exact_path_type_uses_path_matcher() {
        let ing = ingress(vec![IngressRule::new(
            Some("example.com"),
            vec![HttpPath::new(Some("/exact"), PathType::Exact, IngressBackend::numeric("a", 80))],
        )]);
        let t = ing.translate().unwrap();
        assert_eq!(t.routers[0].rule_text, "Host(`example.com`) && Path(`/exact`)");
    }

    #[test]
    fn implementation_specific_path_type_uses_prefix() {
        let ing = ingress(vec![IngressRule::new(
            Some("example.com"),
            vec![HttpPath::new(
                Some("/x"),
                PathType::ImplementationSpecific,
                IngressBackend::numeric("a", 80),
            )],
        )]);
        let t = ing.translate().unwrap();
        assert_eq!(t.routers[0].rule_text, "Host(`example.com`) && PathPrefix(`/x`)");
    }

    #[test]
    fn host_only_rule_omits_path_matcher() {
        let ing = ingress(vec![IngressRule::new(
            Some("example.com"),
            vec![HttpPath::new(None, PathType::Prefix, IngressBackend::numeric("a", 80))],
        )]);
        let t = ing.translate().unwrap();
        assert_eq!(t.routers[0].rule_text, "Host(`example.com`)");
    }

    #[test]
    fn path_only_rule_omits_host_matcher() {
        let ing = ingress(vec![IngressRule::new(
            None,
            vec![prefix_path("/only", IngressBackend::numeric("a", 80))],
        )]);
        let t = ing.translate().unwrap();
        assert_eq!(t.routers[0].rule_text, "PathPrefix(`/only`)");
    }

    #[test]
    fn empty_rule_is_rejected() {
        let ing = ingress(vec![IngressRule::new(
            None,
            vec![HttpPath::new(None, PathType::Prefix, IngressBackend::numeric("a", 80))],
        )]);
        assert_eq!(
            ing.translate().unwrap_err(),
            IngressError::EmptyRule { ingress: "default/web".to_string() }
        );
    }

    #[test]
    fn empty_service_name_is_rejected() {
        let ing = ingress(vec![IngressRule::new(
            Some("a.com"),
            vec![prefix_path("/", IngressBackend::numeric("", 80))],
        )]);
        assert_eq!(ing.translate().unwrap_err(), IngressError::EmptyServiceName);
    }

    #[test]
    fn zero_port_is_rejected() {
        let ing = ingress(vec![IngressRule::new(
            Some("a.com"),
            vec![prefix_path("/", IngressBackend::numeric("api", 0))],
        )]);
        assert_eq!(ing.translate().unwrap_err(), IngressError::InvalidPort("api".to_string()));
    }

    #[test]
    fn host_with_backtick_is_reported_not_panicked() {
        // A host carrying a backtick would break the `Host(`...`)` rule text;
        // the translation surfaces it as an error rather than panicking.
        let ing = ingress(vec![IngressRule::new(
            Some("ev`il.com"),
            vec![prefix_path("/", IngressBackend::numeric("a", 80))],
        )]);
        match ing.translate().unwrap_err() {
            IngressError::InvalidRule { ingress, .. } => assert_eq!(ingress, "default/web"),
            other => panic!("expected InvalidRule, got {other:?}"),
        }
    }

    #[test]
    fn multiple_paths_make_distinct_routers_and_dedup_backends() {
        let ing = ingress(vec![IngressRule::new(
            Some("example.com"),
            vec![
                prefix_path("/a", IngressBackend::numeric("svc", 80)),
                prefix_path("/b", IngressBackend::numeric("svc", 80)),
            ],
        )]);
        let t = ing.translate().unwrap();
        assert_eq!(t.routers.len(), 2);
        assert_ne!(t.routers[0].name, t.routers[1].name);
        // Both paths point at the same service -> one deduped backend.
        assert_eq!(t.backends.len(), 1);
        assert_eq!(t.backends[0].service_id, "default-svc-80");
    }

    #[test]
    fn tls_enabled_for_listed_host_and_wildcard() {
        let mut ing = ingress(vec![
            IngressRule::new(
                Some("secure.com"),
                vec![prefix_path("/", IngressBackend::numeric("a", 80))],
            ),
            IngressRule::new(
                Some("plain.com"),
                vec![prefix_path("/", IngressBackend::numeric("a", 80))],
            ),
        ]);
        ing.tls = vec![IngressTls { hosts: vec!["secure.com".to_string()], secret_name: None }];
        let t = ing.translate().unwrap();
        assert!(t.routers[0].tls, "secure.com is in the tls host list");
        assert!(!t.routers[1].tls, "plain.com is not");

        // An empty host list is a wildcard covering every router.
        ing.tls = vec![IngressTls { hosts: vec![], secret_name: None }];
        let t = ing.translate().unwrap();
        assert!(t.routers[0].tls && t.routers[1].tls);
    }

    #[test]
    fn default_backend_makes_lowest_priority_catch_all() {
        let mut ing = ingress(vec![IngressRule::new(
            Some("example.com"),
            vec![prefix_path("/api", IngressBackend::numeric("api", 80))],
        )]);
        ing.default_backend = Some(IngressBackend::numeric("fallback", 80));
        let t = ing.translate().unwrap();
        let def = t.routers.iter().find(|r| r.name == "default-web-default").unwrap();
        assert_eq!(def.rule_text, "PathPrefix(`/`)");
        assert_eq!(def.priority, Some(1));
        assert_eq!(def.service, "default-fallback-80");
    }

    #[test]
    fn translate_ingresses_filters_by_class_and_merges() {
        let mut mine = ingress(vec![IngressRule::new(
            Some("a.com"),
            vec![prefix_path("/", IngressBackend::numeric("a", 80))],
        )]);
        mine.ingress_class_name = Some("traefik".to_string());
        let mut theirs = ingress(vec![IngressRule::new(
            Some("b.com"),
            vec![prefix_path("/", IngressBackend::numeric("b", 80))],
        )]);
        theirs.name = "other".to_string();
        theirs.ingress_class_name = Some("nginx".to_string());

        let t = translate_ingresses(&[mine, theirs], Some("traefik")).unwrap();
        assert_eq!(t.routers.len(), 1);
        assert_eq!(t.routers[0].rule_text, "Host(`a.com`) && PathPrefix(`/`)");
    }

    #[test]
    fn into_config_builds_routable_config() {
        let ing = ingress(vec![IngressRule::new(
            Some("example.com"),
            vec![prefix_path("/api", IngressBackend::numeric("api", 80))],
        )]);
        let cfg = ing
            .translate()
            .unwrap()
            .into_config(|_b| vec![Server::new("http://10.0.0.1:80")])
            .unwrap();
        let req = RequestDescriptor::new("GET", "http", "example.com", "/api/users");
        let route = cfg.route(&req, None).unwrap();
        assert_eq!(route.service.name, "default-api-80");
        assert_eq!(route.service.pick_round_robin(0).unwrap().url, "http://10.0.0.1:80");
    }

    #[test]
    fn into_config_rejects_backend_with_no_endpoints() {
        let ing = ingress(vec![IngressRule::new(
            Some("example.com"),
            vec![prefix_path("/api", IngressBackend::numeric("api", 80))],
        )]);
        let err = ing.translate().unwrap().into_config(|_b| vec![]).unwrap_err();
        assert_eq!(err, ConfigError::EmptyService("default-api-80".to_string()));
    }
}
