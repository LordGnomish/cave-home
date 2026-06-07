// SPDX-License-Identifier: Apache-2.0
//! Bridge between live `hyper`/`http` wire types and the routing-decision
//! core's [`RequestDescriptor`] / [`ResponseDescriptor`].
//!
//! The decision core (`rule` / `matcher` / `router` / `middleware` / …) is
//! deliberately I/O-free and reasons over descriptors. This module is the thin,
//! **pure** adapter that turns an `http::Request` into the [`RequestDescriptor`]
//! the core routes, and turns a core [`ResponseDescriptor`] (e.g. a middleware
//! redirect) back into the status + headers of an `http::Response`.
//!
//! Everything here is synchronous and allocation-light so it is unit-testable
//! without a socket; the async listener in [`crate::server`] calls it per
//! request.

use http::header::{HeaderMap, HeaderName, HeaderValue};
use http::{Method, StatusCode, Uri};

use crate::request::{RequestDescriptor, ResponseDescriptor};

/// Strip a `:port` suffix from an authority, leaving the host.
///
/// Handles bracketed IPv6 literals (`[::1]:8080` → `[::1]`) so the colon inside
/// the address is not mistaken for the port separator.
#[must_use]
pub fn host_without_port(authority: &str) -> &str {
    if authority.starts_with('[') {
        return authority.find(']').map_or(authority, |idx| &authority[..=idx]);
    }
    authority.find(':').map_or(authority, |idx| &authority[..idx])
}

/// Build a [`RequestDescriptor`] from request parts and the scheme the request
/// arrived on (`"http"` / `"https"`, decided by the listening entrypoint).
///
/// Host resolution follows HTTP/1.1: the `Host` header wins, falling back to
/// the URI authority. The port is stripped so the value matches `Host(`…`)`
/// rules. Duplicate header lines are joined with `, ` per RFC 9110 §5.2.
#[must_use]
pub fn descriptor_from_parts(
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    scheme: &str,
) -> RequestDescriptor {
    let host_src = headers
        .get(http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .or_else(|| uri.authority().map(|a| a.as_str().to_owned()))
        .unwrap_or_default();
    let host = host_without_port(host_src.trim());

    let mut desc = RequestDescriptor::new(method.as_str(), scheme, host, uri.path());
    for (name, value) in headers {
        let Ok(v) = value.to_str() else { continue };
        let key = name.as_str().to_ascii_lowercase();
        desc.headers
            .entry(key)
            .and_modify(|existing| {
                existing.push_str(", ");
                existing.push_str(v);
            })
            .or_insert_with(|| v.to_owned());
    }
    desc
}

/// Build a [`RequestDescriptor`] from a borrowed `http::Request`.
#[must_use]
pub fn descriptor_from_request<B>(req: &http::Request<B>, scheme: &str) -> RequestDescriptor {
    descriptor_from_parts(req.method(), req.uri(), req.headers(), scheme)
}

/// Extract a named cookie value from the request's `Cookie` header(s).
#[must_use]
pub fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    for raw in headers.get_all(http::header::COOKIE) {
        let Ok(s) = raw.to_str() else { continue };
        for pair in s.split(';') {
            if let Some((k, v)) = pair.trim().split_once('=') {
                if k.trim() == name {
                    return Some(v.trim().to_owned());
                }
            }
        }
    }
    None
}

/// Render a core [`ResponseDescriptor`] into a short-circuit response's parts.
///
/// Used for terminal middlewares (e.g. a redirect). Headers that cannot be
/// represented on the wire are skipped rather than failing the whole response.
#[must_use]
pub fn short_circuit_parts(resp: &ResponseDescriptor) -> (StatusCode, HeaderMap) {
    let status = resp
        .status
        .and_then(|c| StatusCode::from_u16(c).ok())
        .unwrap_or(StatusCode::OK);
    let mut map = HeaderMap::new();
    apply_response_headers(&mut map, resp);
    (status, map)
}

/// Merge a [`ResponseDescriptor`]'s headers onto an existing response header
/// map (used when a non-terminal middleware set response headers that must ride
/// back with the proxied response).
pub fn apply_response_headers(dst: &mut HeaderMap, resp: &ResponseDescriptor) {
    for (k, v) in &resp.headers {
        if let (Ok(name), Ok(val)) =
            (HeaderName::from_bytes(k.as_bytes()), HeaderValue::from_str(v))
        {
            dst.insert(name, val);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hv(s: &str) -> HeaderValue {
        HeaderValue::from_str(s).unwrap()
    }

    #[test]
    fn host_without_port_strips_port() {
        assert_eq!(host_without_port("example.com:8080"), "example.com");
        assert_eq!(host_without_port("example.com"), "example.com");
    }

    #[test]
    fn host_without_port_handles_ipv6_literal() {
        assert_eq!(host_without_port("[::1]:8080"), "[::1]");
        assert_eq!(host_without_port("[2001:db8::1]"), "[2001:db8::1]");
    }

    #[test]
    fn descriptor_extracts_method_path_scheme() {
        let mut h = HeaderMap::new();
        h.insert(http::header::HOST, hv("Example.COM:443"));
        let uri: Uri = "/api/users?q=1".parse().unwrap();
        let d = descriptor_from_parts(&Method::POST, &uri, &h, "https");
        assert_eq!(d.method, "POST");
        assert_eq!(d.host, "example.com"); // host header wins, port stripped, lower-cased
        assert_eq!(d.path, "/api/users"); // query excluded
        assert_eq!(d.scheme, "https");
    }

    #[test]
    fn descriptor_host_header_beats_uri_authority() {
        let mut h = HeaderMap::new();
        h.insert(http::header::HOST, hv("from-header.com"));
        let uri: Uri = "http://from-uri.com/x".parse().unwrap();
        let d = descriptor_from_parts(&Method::GET, &uri, &h, "http");
        assert_eq!(d.host, "from-header.com");
    }

    #[test]
    fn descriptor_lowercases_and_joins_duplicate_headers() {
        let mut h = HeaderMap::new();
        h.insert(http::header::HOST, hv("h"));
        h.append(HeaderName::from_static("x-tag"), hv("a"));
        h.append(HeaderName::from_static("x-tag"), hv("b"));
        let uri: Uri = "/".parse().unwrap();
        let d = descriptor_from_parts(&Method::GET, &uri, &h, "http");
        assert_eq!(d.header("X-Tag"), Some("a, b"));
    }

    #[test]
    fn cookie_value_finds_named_cookie() {
        let mut h = HeaderMap::new();
        h.insert(http::header::COOKIE, hv("sid=abc; srv=http://b:80; theme=dark"));
        assert_eq!(cookie_value(&h, "srv").as_deref(), Some("http://b:80"));
        assert_eq!(cookie_value(&h, "missing"), None);
    }

    #[test]
    fn short_circuit_parts_carries_status_and_location() {
        let mut resp = ResponseDescriptor::default();
        resp.status = Some(302);
        resp.headers.insert("location".to_string(), "https://x/y".to_string());
        let (status, headers) = short_circuit_parts(&resp);
        assert_eq!(status, StatusCode::FOUND);
        assert_eq!(headers.get("location").unwrap(), "https://x/y");
    }

    #[test]
    fn apply_response_headers_merges_onto_existing() {
        let mut dst = HeaderMap::new();
        dst.insert(HeaderName::from_static("x-keep"), hv("1"));
        let mut resp = ResponseDescriptor::default();
        resp.headers.insert("x-frame-options".to_string(), "DENY".to_string());
        apply_response_headers(&mut dst, &resp);
        assert_eq!(dst.get("x-keep").unwrap(), "1");
        assert_eq!(dst.get("x-frame-options").unwrap(), "DENY");
    }
}
