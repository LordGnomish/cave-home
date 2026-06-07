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

// Implementation lands in the GREEN commit; this RED commit ships only the
// failing test suite that specifies the translation contract.

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
