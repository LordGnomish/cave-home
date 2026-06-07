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

// Implementation lands in the GREEN commit; this RED commit ships only the
// failing test suite that specifies the HTTPRoute translation contract.

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
