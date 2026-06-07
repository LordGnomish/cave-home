// SPDX-License-Identifier: Apache-2.0
//! CORS handling (the runtime half of cross-origin middleware).
//!
//! Spec basis: the Fetch CORS protocol as exposed by Traefik's `Headers`
//! middleware `accessControlAllow*` options — preflight (`OPTIONS` carrying
//! `Access-Control-Request-Method`) gets a short-circuit response with the
//! `Access-Control-Allow-*` headers; an actual cross-origin request gets the
//! allow-origin / allow-credentials / expose-headers decoration.

/// Which origins are permitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origins {
    /// Any origin (`*`).
    Any,
    /// An explicit allow-list (compared case-sensitively, as origins are).
    List(Vec<String>),
}

/// A CORS policy.
#[derive(Debug, Clone)]
pub struct CorsPolicy {
    /// Permitted origins.
    pub allow_origins: Origins,
    /// Methods advertised in preflight responses.
    pub allow_methods: Vec<String>,
    /// Request headers advertised in preflight responses.
    pub allow_headers: Vec<String>,
    /// Response headers exposed to the browser.
    pub expose_headers: Vec<String>,
    /// Whether credentials are allowed (forces an echoed origin, never `*`).
    pub allow_credentials: bool,
    /// Preflight cache lifetime in seconds.
    pub max_age_secs: u64,
}

/// Whether a request is a CORS preflight: `OPTIONS` with an
/// `Access-Control-Request-Method` header present.
#[must_use]
pub fn is_preflight(method: &str, has_request_method_header: bool) -> bool {
    unimplemented!()
}

impl CorsPolicy {
    /// Whether `origin` is permitted by this policy.
    #[must_use]
    pub fn allows(&self, origin: &str) -> bool {
        unimplemented!()
    }

    /// The `Access-Control-Allow-Origin` value to send for `origin`, or `None`
    /// if the origin is not allowed. With credentials the origin is echoed
    /// (never `*`).
    #[must_use]
    pub fn allow_origin_value(&self, origin: &str) -> Option<String> {
        unimplemented!()
    }

    /// Headers for a preflight (`204`) response, or empty if `origin` is denied.
    #[must_use]
    pub fn preflight_headers(&self, origin: &str) -> Vec<(String, String)> {
        unimplemented!()
    }

    /// Headers to decorate an actual cross-origin response, or empty if denied.
    #[must_use]
    pub fn actual_headers(&self, origin: &str) -> Vec<(String, String)> {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> CorsPolicy {
        CorsPolicy {
            allow_origins: Origins::List(vec!["https://app.example".to_string()]),
            allow_methods: vec!["GET".to_string(), "POST".to_string()],
            allow_headers: vec!["Authorization".to_string(), "Content-Type".to_string()],
            expose_headers: vec!["X-Total".to_string()],
            allow_credentials: true,
            max_age_secs: 600,
        }
    }

    fn get<'a>(h: &'a [(String, String)], k: &str) -> Option<&'a str> {
        h.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str())
    }

    #[test]
    fn detects_preflight() {
        assert!(is_preflight("OPTIONS", true));
        assert!(!is_preflight("OPTIONS", false));
        assert!(!is_preflight("GET", true));
    }

    #[test]
    fn origin_allow_list_is_enforced() {
        let p = policy();
        assert!(p.allows("https://app.example"));
        assert!(!p.allows("https://evil.example"));
    }

    #[test]
    fn any_origin_without_credentials_is_wildcard() {
        let p = CorsPolicy {
            allow_origins: Origins::Any,
            allow_credentials: false,
            ..policy()
        };
        assert_eq!(p.allow_origin_value("https://x").as_deref(), Some("*"));
    }

    #[test]
    fn credentials_force_echoed_origin() {
        let p = policy(); // credentials = true, list includes app.example
        assert_eq!(
            p.allow_origin_value("https://app.example").as_deref(),
            Some("https://app.example")
        );
        assert_eq!(p.allow_origin_value("https://evil.example"), None);
    }

    #[test]
    fn preflight_advertises_methods_headers_and_max_age() {
        let h = policy().preflight_headers("https://app.example");
        assert_eq!(get(&h, "access-control-allow-origin"), Some("https://app.example"));
        assert_eq!(get(&h, "access-control-allow-methods"), Some("GET, POST"));
        assert_eq!(get(&h, "access-control-allow-headers"), Some("Authorization, Content-Type"));
        assert_eq!(get(&h, "access-control-allow-credentials"), Some("true"));
        assert_eq!(get(&h, "access-control-max-age"), Some("600"));
    }

    #[test]
    fn denied_origin_gets_no_headers() {
        assert!(policy().preflight_headers("https://evil.example").is_empty());
        assert!(policy().actual_headers("https://evil.example").is_empty());
    }

    #[test]
    fn actual_response_exposes_headers_and_credentials() {
        let h = policy().actual_headers("https://app.example");
        assert_eq!(get(&h, "access-control-allow-origin"), Some("https://app.example"));
        assert_eq!(get(&h, "access-control-expose-headers"), Some("X-Total"));
        assert_eq!(get(&h, "access-control-allow-credentials"), Some("true"));
    }
}
