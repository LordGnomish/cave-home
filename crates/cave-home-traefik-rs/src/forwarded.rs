// SPDX-License-Identifier: Apache-2.0
//! Hop-by-hop header handling and the `X-Forwarded-*` set a reverse proxy adds.
//!
//! Spec basis: RFC 9110 / RFC 7230 §6.1 (`Connection` and the connection-token
//! mechanism), the standard list of hop-by-hop headers, and Traefik's
//! documented forwarding behaviour: it appends the client to `X-Forwarded-For`,
//! and sets `X-Forwarded-Proto` / `X-Forwarded-Host` / `X-Forwarded-Port` /
//! `X-Real-Ip` to describe the connection as it entered the proxy.

use http::header::{HeaderMap, HeaderName, HeaderValue};

/// The standard end-to-end-breaking headers that must not be forwarded to the
/// backend (RFC 7230 §6.1). Compared case-insensitively.
const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// Whether `name` is one of the fixed hop-by-hop headers.
#[must_use]
pub fn is_hop_by_hop(name: &str) -> bool {
    unimplemented!()
}

/// Build the next `X-Forwarded-For` value: the existing chain (if any) with
/// `client_ip` appended, comma-separated per RFC 7239 conventions.
#[must_use]
pub fn xff_chain(existing: Option<&str>, client_ip: &str) -> String {
    unimplemented!()
}

/// Remove every hop-by-hop header from `headers`, including the per-connection
/// headers named as tokens in the `Connection` header itself (RFC 7230 §6.1).
pub fn strip_hop_by_hop(headers: &mut HeaderMap) {
    unimplemented!()
}

/// A description of the inbound connection, used to set the `X-Forwarded-*`
/// headers on the upstream request.
#[derive(Debug, Clone)]
pub struct Forwarded {
    /// The immediate client's IP (textual).
    pub client_ip: String,
    /// The scheme the request arrived on (`http` / `https`).
    pub proto: String,
    /// The `Host` the request targeted (may include a port).
    pub host: String,
    /// The port the request arrived on, if known.
    pub port: Option<u16>,
}

impl Forwarded {
    /// Set / append the `X-Forwarded-*` and `X-Real-Ip` headers on `headers`.
    pub fn apply(&self, headers: &mut HeaderMap) {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hv(s: &str) -> HeaderValue {
        HeaderValue::from_str(s).unwrap()
    }

    #[test]
    fn hop_by_hop_recognises_standard_set_case_insensitively() {
        assert!(is_hop_by_hop("connection"));
        assert!(is_hop_by_hop("Connection"));
        assert!(is_hop_by_hop("Transfer-Encoding"));
        assert!(is_hop_by_hop("upgrade"));
        assert!(is_hop_by_hop("TE"));
        assert!(!is_hop_by_hop("content-type"));
        assert!(!is_hop_by_hop("x-forwarded-for"));
    }

    #[test]
    fn xff_chain_starts_or_appends() {
        assert_eq!(xff_chain(None, "2.2.2.2"), "2.2.2.2");
        assert_eq!(xff_chain(Some("1.1.1.1"), "2.2.2.2"), "1.1.1.1, 2.2.2.2");
    }

    #[test]
    fn strip_removes_fixed_hop_by_hop_keeps_others() {
        let mut h = HeaderMap::new();
        h.insert(http::header::CONNECTION, hv("keep-alive"));
        h.insert(http::header::UPGRADE, hv("websocket"));
        h.insert(http::header::CONTENT_TYPE, hv("application/json"));
        strip_hop_by_hop(&mut h);
        assert!(!h.contains_key(http::header::CONNECTION));
        assert!(!h.contains_key(http::header::UPGRADE));
        assert_eq!(h.get(http::header::CONTENT_TYPE).unwrap(), "application/json");
    }

    #[test]
    fn strip_removes_headers_named_in_connection_token_list() {
        let mut h = HeaderMap::new();
        h.insert(http::header::CONNECTION, hv("X-Custom, close"));
        h.insert(HeaderName::from_static("x-custom"), hv("secret"));
        h.insert(HeaderName::from_static("x-keep"), hv("yes"));
        strip_hop_by_hop(&mut h);
        assert!(!h.contains_key("x-custom"));
        assert_eq!(h.get("x-keep").unwrap(), "yes");
    }

    #[test]
    fn forwarded_sets_all_headers_and_appends_xff() {
        let mut h = HeaderMap::new();
        h.insert(HeaderName::from_static("x-forwarded-for"), hv("9.9.9.9"));
        let fwd = Forwarded {
            client_ip: "2.2.2.2".to_string(),
            proto: "https".to_string(),
            host: "example.com".to_string(),
            port: Some(443),
        };
        fwd.apply(&mut h);
        assert_eq!(h.get("x-forwarded-for").unwrap(), "9.9.9.9, 2.2.2.2");
        assert_eq!(h.get("x-forwarded-proto").unwrap(), "https");
        assert_eq!(h.get("x-forwarded-host").unwrap(), "example.com");
        assert_eq!(h.get("x-forwarded-port").unwrap(), "443");
        assert_eq!(h.get("x-real-ip").unwrap(), "2.2.2.2");
    }
}
