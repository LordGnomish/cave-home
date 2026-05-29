// SPDX-License-Identifier: Apache-2.0
//! Middleware chain model.
//!
//! A [`Middleware`] is a typed transform applied to the request (and, for some,
//! the response) before the request reaches its backend service. A
//! [`MiddlewareChain`] applies an ordered list in sequence, short-circuiting
//! when a middleware produces a terminal response (e.g. a redirect).
//!
//! Spec basis (public Traefik middlewares docs):
//! * `StripPrefix` removes a matching leading path segment.
//! * `AddPrefix` prepends a path segment.
//! * `RedirectScheme` issues a redirect to another scheme (e.g. http→https).
//! * `Headers` sets custom request/response headers.
//! * `BasicAuth` / `RateLimit` are modelled as *configuration* only — the
//!   credential crypto and the token-bucket clock are deferred to phase-1b.

use crate::request::{RequestDescriptor, ResponseDescriptor};

/// One middleware in a chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Middleware {
    /// Remove the first matching prefix from the request path.
    StripPrefix {
        /// Prefixes to try, in order; the first that matches is stripped.
        prefixes: Vec<String>,
    },
    /// Prepend `prefix` to the request path.
    AddPrefix {
        /// Prefix to add (e.g. `/api`).
        prefix: String,
    },
    /// Redirect to a different scheme.
    RedirectScheme {
        /// Target scheme (e.g. `https`).
        scheme: String,
        /// Optional explicit port for the redirect target.
        port: Option<u16>,
        /// Whether to use a 301 (permanent) instead of 302 (temporary).
        permanent: bool,
    },
    /// Set request and/or response headers.
    Headers {
        /// Headers to set on the request before it reaches the backend.
        request: Vec<(String, String)>,
        /// Headers to set on the response.
        response: Vec<(String, String)>,
    },
    /// Basic-auth *configuration*. The credential check (constant-time hash
    /// compare) is deferred; this models the realm + user list reference.
    BasicAuth {
        /// Authentication realm presented in the challenge.
        realm: String,
        /// Opaque references to the credential source (e.g. htpasswd lines).
        users: Vec<String>,
    },
    /// Rate-limit *configuration*. The token-bucket runtime is deferred; this
    /// models the configured average/burst the runtime will enforce.
    RateLimit {
        /// Sustained average requests per second.
        average: u64,
        /// Burst capacity.
        burst: u64,
    },
}

/// The result of applying a chain: the (possibly transformed) request, the
/// accumulated response state, and whether the chain short-circuited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// The transformed request descriptor.
    pub request: RequestDescriptor,
    /// The accumulated response descriptor.
    pub response: ResponseDescriptor,
    /// `true` if a middleware produced a terminal response and the remaining
    /// middlewares + the backend were skipped.
    pub short_circuited: bool,
}

impl Middleware {
    /// Apply this single middleware, mutating `req`/`resp` in place. Returns
    /// `true` if the chain should short-circuit after this middleware.
    fn apply_one(&self, req: &mut RequestDescriptor, resp: &mut ResponseDescriptor) -> bool {
        match self {
            Self::StripPrefix { prefixes } => {
                for p in prefixes {
                    if let Some(stripped) = strip_prefix_segment(&req.path, p) {
                        req.path = stripped;
                        break;
                    }
                }
                false
            }
            Self::AddPrefix { prefix } => {
                req.path = format!("{}{}", prefix, req.path);
                false
            }
            Self::RedirectScheme { scheme, port, permanent } => {
                let authority = match port {
                    Some(p) => format!("{}:{}", host_only(&req.host), p),
                    None => req.host.clone(),
                };
                let location = format!("{}://{}{}", scheme, authority, req.path);
                resp.headers.insert("location".to_string(), location);
                resp.status = Some(if *permanent { 301 } else { 302 });
                true
            }
            Self::Headers { request, response } => {
                for (k, v) in request {
                    req.headers.insert(k.to_ascii_lowercase(), v.clone());
                }
                for (k, v) in response {
                    resp.headers.insert(k.to_ascii_lowercase(), v.clone());
                }
                false
            }
            // Config-only middlewares: no transform in the decision core. Their
            // enforcement (auth check, rate limiting) is a phase-1b runtime job.
            Self::BasicAuth { .. } | Self::RateLimit { .. } => false,
        }
    }
}

/// An ordered set of middlewares applied before the backend service.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MiddlewareChain {
    /// The middlewares, applied front-to-back.
    pub middlewares: Vec<Middleware>,
}

impl MiddlewareChain {
    /// Build a chain from an ordered list of middlewares.
    #[must_use]
    pub const fn new(middlewares: Vec<Middleware>) -> Self {
        Self { middlewares }
    }

    /// Apply the chain to `req`, returning the transformed request, the
    /// accumulated response, and whether a middleware short-circuited (in which
    /// case later middlewares were skipped).
    #[must_use]
    pub fn apply(&self, req: RequestDescriptor) -> Applied {
        let mut request = req;
        let mut response = ResponseDescriptor::default();
        let mut short_circuited = false;
        for mw in &self.middlewares {
            if mw.apply_one(&mut request, &mut response) {
                short_circuited = true;
                break;
            }
        }
        Applied { request, response, short_circuited }
    }
}

/// Strip `prefix` from `path` only on a segment boundary, returning the new
/// path. `/api` stripped from `/api/users` yields `/users`; from `/api` yields
/// `/`. Returns `None` if `prefix` does not match.
fn strip_prefix_segment(path: &str, prefix: &str) -> Option<String> {
    if !path.starts_with(prefix) {
        return None;
    }
    let rest = &path[prefix.len()..];
    if rest.is_empty() {
        Some("/".to_string())
    } else if rest.starts_with('/') {
        Some(rest.to_string())
    } else if prefix.ends_with('/') {
        Some(format!("/{rest}"))
    } else {
        None
    }
}

/// Strip a `:port` suffix from a host, leaving just the host part.
fn host_only(host: &str) -> &str {
    host.split(':').next().unwrap_or(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(path: &str) -> RequestDescriptor {
        RequestDescriptor::new("GET", "http", "example.com", path)
    }

    #[test]
    fn strip_prefix_removes_leading_segment() {
        let mw = Middleware::StripPrefix { prefixes: vec!["/api".to_string()] };
        let out = MiddlewareChain::new(vec![mw]).apply(req("/api/users"));
        assert_eq!(out.request.path, "/users");
        assert!(!out.short_circuited);
    }

    #[test]
    fn strip_prefix_exact_becomes_root() {
        let mw = Middleware::StripPrefix { prefixes: vec!["/api".to_string()] };
        let out = MiddlewareChain::new(vec![mw]).apply(req("/api"));
        assert_eq!(out.request.path, "/");
    }

    #[test]
    fn strip_prefix_no_match_leaves_path() {
        let mw = Middleware::StripPrefix { prefixes: vec!["/api".to_string()] };
        let out = MiddlewareChain::new(vec![mw]).apply(req("/apixyz"));
        assert_eq!(out.request.path, "/apixyz");
    }

    #[test]
    fn strip_prefix_tries_alternatives_in_order() {
        let mw = Middleware::StripPrefix {
            prefixes: vec!["/v1".to_string(), "/api".to_string()],
        };
        let out = MiddlewareChain::new(vec![mw]).apply(req("/api/x"));
        assert_eq!(out.request.path, "/x");
    }

    #[test]
    fn add_prefix_prepends() {
        let mw = Middleware::AddPrefix { prefix: "/api".to_string() };
        let out = MiddlewareChain::new(vec![mw]).apply(req("/users"));
        assert_eq!(out.request.path, "/api/users");
    }

    #[test]
    fn strip_then_add_compose_in_order() {
        let chain = MiddlewareChain::new(vec![
            Middleware::StripPrefix { prefixes: vec!["/old".to_string()] },
            Middleware::AddPrefix { prefix: "/new".to_string() },
        ]);
        let out = chain.apply(req("/old/thing"));
        assert_eq!(out.request.path, "/new/thing");
    }

    #[test]
    fn redirect_scheme_short_circuits_with_location_and_302() {
        let mw = Middleware::RedirectScheme {
            scheme: "https".to_string(),
            port: None,
            permanent: false,
        };
        let out = MiddlewareChain::new(vec![mw]).apply(req("/secure"));
        assert!(out.short_circuited);
        assert_eq!(out.response.status, Some(302));
        assert_eq!(out.response.header("location"), Some("https://example.com/secure"));
    }

    #[test]
    fn redirect_scheme_permanent_uses_301_and_port() {
        let mw = Middleware::RedirectScheme {
            scheme: "https".to_string(),
            port: Some(8443),
            permanent: true,
        };
        let out = MiddlewareChain::new(vec![mw]).apply(req("/x"));
        assert_eq!(out.response.status, Some(301));
        assert_eq!(out.response.header("location"), Some("https://example.com:8443/x"));
    }

    #[test]
    fn redirect_short_circuit_skips_later_middleware() {
        let chain = MiddlewareChain::new(vec![
            Middleware::RedirectScheme {
                scheme: "https".to_string(),
                port: None,
                permanent: false,
            },
            Middleware::AddPrefix { prefix: "/should-not-apply".to_string() },
        ]);
        let out = chain.apply(req("/x"));
        // AddPrefix must NOT have run.
        assert_eq!(out.request.path, "/x");
    }

    #[test]
    fn headers_set_request_and_response() {
        let mw = Middleware::Headers {
            request: vec![("X-Real-IP".to_string(), "1.2.3.4".to_string())],
            response: vec![("X-Frame-Options".to_string(), "DENY".to_string())],
        };
        let out = MiddlewareChain::new(vec![mw]).apply(req("/"));
        assert_eq!(out.request.header("x-real-ip"), Some("1.2.3.4"));
        assert_eq!(out.response.header("x-frame-options"), Some("DENY"));
    }

    #[test]
    fn config_only_middlewares_are_pass_through() {
        let chain = MiddlewareChain::new(vec![
            Middleware::BasicAuth { realm: "r".to_string(), users: vec!["u:h".to_string()] },
            Middleware::RateLimit { average: 100, burst: 50 },
        ]);
        let out = chain.apply(req("/x"));
        assert!(!out.short_circuited);
        assert_eq!(out.request.path, "/x");
        assert!(out.response.status.is_none());
    }

    #[test]
    fn empty_chain_is_identity() {
        let out = MiddlewareChain::default().apply(req("/x"));
        assert_eq!(out.request.path, "/x");
        assert!(!out.short_circuited);
    }
}
