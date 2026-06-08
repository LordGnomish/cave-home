// SPDX-License-Identifier: Apache-2.0
//! Incoming request / outgoing response descriptors.
//!
//! These are the values the routing-decision core operates over. They model
//! just enough of an HTTP exchange for routing-rule evaluation and middleware
//! transformation; the wire-level parsing of bytes into these structs is the
//! deferred listener's job (phase-1b — see `parity.manifest.toml`).
//!
//! Spec basis: Traefik routing rules match on the request line (method, path)
//! and headers, and `Host` is matched against the `Host` header / SNI. See the
//! public Traefik HTTP-routers documentation.

use std::collections::BTreeMap;

/// A minimal description of an incoming HTTP request, sufficient for routing.
///
/// Header names are stored lower-cased to give case-insensitive matching, in
/// line with HTTP/1.1 (RFC 9110 §5.1: field names are case-insensitive).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestDescriptor {
    /// HTTP method, upper-cased on construction (e.g. `GET`).
    pub method: String,
    /// The `Host` the request is addressed to (host header / SNI), lower-cased.
    pub host: String,
    /// Request path beginning with `/` (query string excluded).
    pub path: String,
    /// Request headers, keyed by lower-cased name. A name may map to a single
    /// joined value; multi-value headers are joined with `,` per RFC 9110 §5.2.
    pub headers: BTreeMap<String, String>,
    /// URI scheme the request arrived on (`http` / `https`), lower-cased.
    pub scheme: String,
}

impl RequestDescriptor {
    /// Build a request descriptor, normalising method/host/scheme casing.
    #[must_use]
    pub fn new(method: &str, scheme: &str, host: &str, path: &str) -> Self {
        Self {
            method: method.to_ascii_uppercase(),
            host: host.to_ascii_lowercase(),
            path: if path.is_empty() { "/".to_string() } else { path.to_string() },
            headers: BTreeMap::new(),
            scheme: scheme.to_ascii_lowercase(),
        }
    }

    /// Insert a header, normalising the name to lower-case (builder style).
    #[must_use]
    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers
            .insert(name.to_ascii_lowercase(), value.to_string());
        self
    }

    /// Look up a header value by (case-insensitive) name.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_ascii_lowercase()).map(String::as_str)
    }
}

/// The routing decision's effect on the response side.
///
/// Middlewares that short-circuit (e.g. `RedirectScheme`) record their effect
/// here rather than mutating the request.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResponseDescriptor {
    /// Status code the chain wants to return early, if any (e.g. 302 redirect).
    pub status: Option<u16>,
    /// Response headers the chain has set (e.g. `Location`, security headers).
    pub headers: BTreeMap<String, String>,
}

impl ResponseDescriptor {
    /// Look up a response header value by (case-insensitive) name.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_ascii_lowercase()).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_normalises_method_host_scheme_casing() {
        let r = RequestDescriptor::new("get", "HTTPS", "Example.COM", "/a");
        assert_eq!(r.method, "GET");
        assert_eq!(r.host, "example.com");
        assert_eq!(r.scheme, "https");
        assert_eq!(r.path, "/a");
    }

    #[test]
    fn empty_path_becomes_root() {
        let r = RequestDescriptor::new("GET", "http", "h", "");
        assert_eq!(r.path, "/");
    }

    #[test]
    fn header_lookup_is_case_insensitive() {
        let r = RequestDescriptor::new("GET", "http", "h", "/")
            .with_header("X-Forwarded-Proto", "https");
        assert_eq!(r.header("x-forwarded-proto"), Some("https"));
        assert_eq!(r.header("X-FORWARDED-PROTO"), Some("https"));
        assert_eq!(r.header("missing"), None);
    }
}
